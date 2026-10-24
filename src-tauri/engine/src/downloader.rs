//! HTTP 下载核心：Range 断点续传 + 流式 SHA-1 + 重试 + 取消。
//!
//! 流程：
//! 1. 目标已存在且 SHA-1 匹配 → 直接跳过（返回 0）；
//! 2. 存在 `<dest>.part` → 发送 `Range: bytes=<len>-` 请求：
//!    - 206 → 以 part 前缀字节种子初始化 SHA-1，追加写入；
//!    - 200 → 服务器不支持 Range，截断重写；
//!    - 416 → part 可能已完整（大小吻合则直接校验改名），否则删除重下；
//! 3. 流式写入时同步计算 SHA-1；完成后校验：
//!    - 通过 → 原子 rename 到目标路径；
//!    - 失败 → 删除 part 重试（最多 3 次），仍失败报 `Sha1Mismatch`；
//! 4. 网络错误/5xx/429 → 指数退避重试；取消令牌触发立即返回 `Canceled`。

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::{EngineError, Result};

pub const MAX_ATTEMPTS: u32 = 3;

pub fn build_http_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("UnknownMinecraftDown/0.2 (downloader)")
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .pool_max_idle_per_host(32)
        .build()
}

pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

pub async fn hash_file(path: &Path) -> Result<String> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 下载单个文件。返回本次调用实际从网络接收的字节数（续传前缀不计入）。
/// `progress` 在每个数据块写入后被调用（参数为本块字节数）。
pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    size_hint: Option<u64>,
    cancel: &CancellationToken,
    progress: &(dyn Fn(u64) + Send + Sync),
) -> Result<u64> {
    if cancel.is_cancelled() {
        return Err(EngineError::Canceled);
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // 快路径：目标已存在且校验通过
    if dest.exists() {
        if let Some(expect) = expected_sha1 {
            if hash_file(dest).await? == expect.to_lowercase() {
                return Ok(0);
            }
        } else {
            return Ok(0); // 无校验值时信任已存在文件
        }
    }

    let part = part_path(dest);
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        let result = try_once(client, url, &part, size_hint, cancel, progress).await;
        match result {
            Ok((bytes_now, sha)) => {
                if let Some(expect) = expected_sha1 {
                    if sha != expect.to_lowercase() {
                        let _ = tokio::fs::remove_file(&part).await;
                        if attempt < MAX_ATTEMPTS {
                            log::warn!("SHA-1 不匹配，重试({attempt}/{MAX_ATTEMPTS}): {url}");
                            tokio::time::sleep(Duration::from_millis(300 * u64::from(attempt)))
                                .await;
                            continue;
                        }
                        return Err(EngineError::Sha1Mismatch {
                            path: dest.display().to_string(),
                            expected: expect.to_lowercase(),
                            actual: sha,
                        });
                    }
                }
                // 原子移动 part -> dest（同文件系统内 rename 是原子的）
                let _ = tokio::fs::remove_file(dest).await;
                tokio::fs::rename(&part, dest).await?;
                return Ok(bytes_now);
            }
            Err(EngineError::Canceled) => {
                // 取消与网络失败都必须保留 .part，重启/重试后从断点续传
                return Err(EngineError::Canceled);
            }
            Err(e) => {
                if attempt < MAX_ATTEMPTS && is_retryable(&e) {
                    log::warn!("下载失败将重试({attempt}/{MAX_ATTEMPTS}) {url}: {e}");
                    tokio::time::sleep(Duration::from_millis(500 * u64::from(attempt))).await;
                    continue;
                }
                return Err(e);
            }
        }
    }
}

fn is_retryable(e: &EngineError) -> bool {
    matches!(e, EngineError::Network(_))
}

/// 单次尝试：返回 (本次网络接收字节, 整个 part 文件的 SHA-1)。
async fn try_once(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    size_hint: Option<u64>,
    cancel: &CancellationToken,
    progress: &(dyn Fn(u64) + Send + Sync),
) -> Result<(u64, String)> {
    let part_len = tokio::fs::metadata(part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let mut req = client.get(url);
    let mut resuming = false;
    if part_len > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={part_len}-"));
        resuming = true;
    }
    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(EngineError::Canceled),
        r = req.send() => r?,
    };
    let status = resp.status();

    // 416：part 比服务器资源还长或恰好完整
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && resuming {
        if let Some(size) = size_hint {
            if part_len == size {
                let sha = hash_file(part).await?;
                return Ok((0, sha));
            }
        }
        let _ = tokio::fs::remove_file(part).await;
        return Err(EngineError::Network(format!(
            "服务器返回 416，已重置 part（{url}）"
        )));
    }

    if status.as_u16() >= 500 || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(EngineError::Network(format!("HTTP {status}（{url}）")));
    }
    if !status.is_success() {
        return Err(EngineError::Network(format!("HTTP {status}（{url}）")));
    }

    // 200：服务器忽略了 Range → 全新下载；206：续传
    let use_resume = resuming && status == reqwest::StatusCode::PARTIAL_CONTENT;

    let mut hasher = Sha1::new();
    if use_resume {
        // 用 part 的已有字节做 SHA-1 种子，保证最终哈希覆盖完整文件
        let mut f = tokio::fs::File::open(part).await?;
        let mut buf = vec![0u8; 128 * 1024];
        loop {
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    }

    let mut file = if use_resume {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(part)
            .await?
    } else {
        tokio::fs::File::create(part).await?
    };

    let mut stream = resp.bytes_stream();
    let mut bytes_now: u64 = 0;
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(EngineError::Canceled),
            c = stream.next() => match c {
                Some(Ok(c)) => c,
                Some(Err(e)) => return Err(EngineError::Network(e.to_string())),
                None => break,
            }
        };
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        bytes_now += chunk.len() as u64;
        progress(chunk.len() as u64);
    }
    file.flush().await?;
    drop(file);

    Ok((bytes_now, hex::encode(hasher.finalize())))
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    //! 极简测试 HTTP 服务器：支持 Range 请求，可模拟中途断流。
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    pub struct TestServer {
        pub addr: SocketAddr,
    }

    pub struct ServerOpts {
        /// 收到 Range 时是否真的支持（false 则忽略 Range 返回 200 全量）
        pub support_range: bool,
        /// 发送前 N 字节后强制断开（模拟中断）
        pub truncate_after: Option<u64>,
        /// 记录收到的 Range 头
        pub seen_ranges: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    pub async fn spawn(body: Vec<u8>, opts: ServerOpts) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let body = body.clone();
                let opts = ServerOpts {
                    support_range: opts.support_range,
                    truncate_after: opts.truncate_after,
                    seen_ranges: opts.seen_ranges.clone(),
                };
                tokio::spawn(async move {
                    let _ = handle(stream, body, opts).await;
                });
            }
        });
        TestServer { addr }
    }

    async fn handle(mut stream: TcpStream, body: Vec<u8>, opts: ServerOpts) -> std::io::Result<()> {
        // 读取请求头
        let mut buf = vec![0u8; 8192];
        let mut read_all = Vec::new();
        let header_end;
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                return Ok(());
            }
            read_all.extend_from_slice(&buf[..n]);
            if let Some(pos) = find_header_end(&read_all) {
                header_end = pos;
                break;
            }
            if read_all.len() > 64 * 1024 {
                return Ok(());
            }
        }
        let head = String::from_utf8_lossy(&read_all[..header_end]).to_string();
        let range = head
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("range")
                    .then(|| v.trim().to_string())
            })
            .unwrap_or_default();
        opts.seen_ranges.lock().unwrap().push(range.clone());

        let mut start = 0u64;
        let mut partial = false;
        if opts.support_range && !range.is_empty() {
            if let Some(r) = range.strip_prefix("bytes=") {
                if let Ok(v) = r.split('-').next().unwrap_or("").parse::<u64>() {
                    start = v.min(body.len() as u64);
                    // start=0 的 Range 视作服务器忽略（按 200 全量），start>0 才是真正的续传
                    partial = start > 0;
                }
            }
        }
        let payload = &body[start as usize..];
        // truncate_after：声明完整 Content-Length 但只发送前 N 字节后断开，
        // 模拟"下载中途断网"（客户端将收到 unexpected EOF）
        let truncated = opts
            .truncate_after
            .map(|t| &payload[..(t as usize).min(payload.len())])
            .unwrap_or(payload);

        let status = if partial {
            "206 Partial Content"
        } else {
            "200 OK"
        };
        let extra = if partial {
            format!(
                "Content-Range: bytes={}-{}/*\r\n",
                start,
                body.len() as u64 - 1
            )
        } else {
            String::new()
        };
        let head_out = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nAccept-Ranges: bytes\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        stream.write_all(head_out.as_bytes()).await?;
        stream.write_all(truncated).await?;
        stream.flush().await?;
        drop(stream);
        Ok(())
    }

    fn find_header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
    }

    pub fn url(addr: SocketAddr, path: &str) -> String {
        format!("http://{addr}{path}")
    }
}
