//! 下载核心集成测试：完整下载、断点续传、SHA-1 失败重试、取消。
//! 全部使用本地回环测试服务器，不依赖外网。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sha1::{Digest, Sha1};
use tokio_util::sync::CancellationToken;

use mc_dl_engine::downloader::{
    build_http_client, download_file, hash_file, part_path, test_support as ts,
};

fn sha1_hex(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(data);
    hex::encode(h.finalize())
}

fn make_payload(len: usize) -> Vec<u8> {
    // 伪随机但可复现的字节流
    let mut v = Vec::with_capacity(len);
    let mut x: u64 = 0x9E3779B97F4A7C15;
    for _ in 0..len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.push(x as u8);
    }
    v
}

fn noop(_d: u64) {}

#[tokio::test]
async fn full_download_writes_file_and_cleans_part() {
    let body = make_payload(10_000);
    let expect = sha1_hex(&body);
    let server = ts::spawn(
        body.clone(),
        ts::ServerOpts {
            support_range: true,
            truncate_after: None,
            seen_ranges: Arc::new(std::sync::Mutex::new(vec![])),
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("sub").join("file.bin");

    let client = build_http_client().unwrap();
    let got = download_file(
        &client,
        &ts::url(server.addr, "/f.bin"),
        &dest,
        Some(&expect),
        Some(body.len() as u64),
        &CancellationToken::new(),
        &noop,
    )
    .await
    .unwrap();
    assert!(got > 0);
    assert!(dest.exists());
    assert!(!part_path(&dest).exists());
    assert_eq!(hash_file(&dest).await.unwrap(), expect);
}

#[tokio::test]
async fn resume_continues_from_part_and_sends_range() {
    let body = make_payload(8_192);
    let expect = sha1_hex(&body);
    // 每次断流只发 1000 字节：引擎最多重试 3 次（共约 3000 字节），
    // 无法在断流服务器上完成整个 8192 字节的文件 → 必然以网络错误收场
    let cut: u64 = 1_000;

    // 第一轮：服务器发送 cut 字节后断流
    let broken = ts::spawn(
        body.clone(),
        ts::ServerOpts {
            support_range: true,
            truncate_after: Some(cut),
            seen_ranges: Arc::new(std::sync::Mutex::new(vec![])),
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("big.bin");
    let client = build_http_client().unwrap();

    let err = download_file(
        &client,
        &ts::url(broken.addr, "/big.bin"),
        &dest,
        Some(&expect),
        Some(body.len() as u64),
        &CancellationToken::new(),
        &noop,
    )
    .await;
    assert!(err.is_err(), "断流应导致失败: {err:?}");
    // .part 已保留断点
    let part_len = std::fs::metadata(part_path(&dest)).unwrap().len();
    assert!(
        part_len >= cut - 128,
        "part 应保留断点字节，实际 {part_len}"
    );

    // 第二轮：完好服务器，应发 Range 从断点续传
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let good = ts::spawn(
        body.clone(),
        ts::ServerOpts {
            support_range: true,
            truncate_after: None,
            seen_ranges: seen.clone(),
        },
    )
    .await;
    let bytes_now = download_file(
        &client,
        &ts::url(good.addr, "/big.bin"),
        &dest,
        Some(&expect),
        Some(body.len() as u64),
        &CancellationToken::new(),
        &noop,
    )
    .await
    .unwrap();
    let ranges = seen.lock().unwrap().clone();
    // 以服务器实际收到的 Range 为准断言“从断点继续而非归零”：
    // （错误路径返回时机与最后一次写入落盘之间没有同步点，测试侧的 stat
    // 快照在极端负载下可能落后于引擎实际使用的偏移，因此不直接比对）
    let resumed_from = ranges
        .iter()
        .filter_map(|r| r.strip_prefix("bytes="))
        .filter_map(|s| s.split('-').next())
        .filter_map(|s| s.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    assert!(
        resumed_from >= cut,
        "续传必须发送 Range 头且起点 ≥ 断点 {cut}，实际收到的请求头: {ranges:?}"
    );
    assert_eq!(
        resumed_from + bytes_now,
        body.len() as u64,
        "断点偏移 + 续传字节 == 总大小"
    );
    assert!(dest.exists());
    assert!(!part_path(&dest).exists());
    assert_eq!(
        hash_file(&dest).await.unwrap(),
        expect,
        "续传后整文件 SHA-1 必须一致"
    );
}

#[tokio::test]
async fn sha1_mismatch_retries_then_fails_and_cleans_part() {
    let real = make_payload(4_096);
    let wrong = sha1_hex(&make_payload(4_097));
    let server = ts::spawn(
        real,
        ts::ServerOpts {
            support_range: true,
            truncate_after: None,
            seen_ranges: Arc::new(std::sync::Mutex::new(vec![])),
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("bad.bin");
    let client = build_http_client().unwrap();

    let err = download_file(
        &client,
        &ts::url(server.addr, "/bad.bin"),
        &dest,
        Some(&wrong),
        None,
        &CancellationToken::new(),
        &noop,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, mc_dl_engine::EngineError::Sha1Mismatch { .. }),
        "{err}"
    );
    // 重试 MAX_ATTEMPTS 次（1 次请求只发生在最后一次失败后清理）
    assert!(!part_path(&dest).exists());
    assert!(!dest.exists());
}

#[tokio::test]
async fn existing_verified_file_is_skipped() {
    let body = make_payload(1_024);
    let expect = sha1_hex(&body);
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("exist.bin");
    std::fs::write(&dest, &body).unwrap();

    let client = build_http_client().unwrap();
    let counter = Arc::new(AtomicU64::new(0));
    let c2 = counter.clone();
    let progress = move |d: u64| {
        c2.fetch_add(d, Ordering::Relaxed);
    };
    let got = download_file(
        &client,
        "http://127.0.0.1:1/never-reached",
        &dest,
        Some(&expect),
        None,
        &CancellationToken::new(),
        &progress,
    )
    .await
    .unwrap();
    assert_eq!(got, 0, "已存在且校验通过的文件不应发起任何网络请求");
    assert_eq!(counter.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn cancel_token_aborts_immediately() {
    let body = make_payload(1_024);
    let server = ts::spawn(
        body,
        ts::ServerOpts {
            support_range: true,
            truncate_after: None,
            seen_ranges: Arc::new(std::sync::Mutex::new(vec![])),
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("c.bin");
    let token = CancellationToken::new();
    token.cancel();
    let client = build_http_client().unwrap();
    let err = download_file(
        &client,
        &ts::url(server.addr, "/c.bin"),
        &dest,
        None,
        None,
        &token,
        &noop,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, mc_dl_engine::EngineError::Canceled));
}
