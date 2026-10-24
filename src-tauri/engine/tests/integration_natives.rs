//! natives 解压防穿越测试：白名单扩展名、exclude 规则、`..` 目录成分清洗、
//! 符号链接逃逸兜底（ensure_within）。

use std::io::Write;

use mc_dl_engine::plan::NativeExtract;
use mc_dl_engine::runner::extract_natives;

fn make_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn extracts_only_native_libs_and_sanitizes_paths() {
    let dir = tempfile::tempdir().unwrap();
    let game = dir.path();
    let jar_rel = "libraries/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-linux.jar";
    std::fs::create_dir_all(game.join("libraries/org/lwjgl/lwjgl/3.3.1")).unwrap();
    make_zip(
        &game.join(jar_rel),
        &[
            ("com/example/inner/libinner.so", b"INNER" as &[u8]),
            ("libgood.so", b"GOOD"),
            ("helper.dll", b"DLL"),
            ("META-INF/MANIFEST.MF", b"manifest"),
            ("readme.txt", b"txt"),
            ("../../../../outside_evil.so", b"EVIL"),
        ],
    );

    let ne = NativeExtract {
        jar: jar_rel.to_string(),
        dest_dir: "versions/test-1.0/natives".to_string(),
        excludes: vec!["META-INF/".to_string()],
    };
    extract_natives(game, &ne).unwrap();

    let natives = game.join("versions/test-1.0/natives");
    // 白名单扩展名内的库被提取（目录成分被丢弃，只保留文件名）
    assert_eq!(
        std::fs::read(natives.join("libinner.so")).unwrap(),
        b"INNER"
    );
    assert_eq!(std::fs::read(natives.join("libgood.so")).unwrap(), b"GOOD");
    assert!(natives.join("helper.dll").exists());
    // 非库文件与 exclude 命中的不提取
    assert!(!natives.join("readme.txt").exists());
    assert!(!natives.join("MANIFEST.MF").exists());
    // `..` 成分被清洗：evil 文件落在 natives 目录内，绝不会写出 game_dir
    assert_eq!(
        std::fs::read(natives.join("outside_evil.so")).unwrap(),
        b"EVIL"
    );
    assert!(!game.join("outside_evil.so").exists());
    assert!(!game.parent().unwrap().join("outside_evil.so").exists());
}

#[test]
fn symlink_escape_is_blocked_by_ensure_within() {
    let dir = tempfile::tempdir().unwrap();
    let game = dir.path();
    // natives 目录本身是符号链接，指向 game_dir 之外
    let outside = tempfile::tempdir().unwrap();
    let versions = game.join("versions/evil/natives");
    std::fs::create_dir_all(game.join("versions/evil")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), &versions).unwrap();

    let jar_rel = "lib.jar";
    make_zip(&game.join(jar_rel), &[("libtest.so", b"X" as &[u8])]);
    let ne = NativeExtract {
        jar: jar_rel.to_string(),
        dest_dir: "versions/evil/natives".to_string(),
        excludes: vec![],
    };
    // ensure_within 应拒绝解压目标越出 game_dir
    let result = extract_natives(game, &ne);
    #[cfg(unix)]
    {
        assert!(result.is_err(), "符号链接逃逸必须被拦截");
        assert!(
            std::fs::read_dir(outside.path()).unwrap().count() == 0,
            "外部目录必须保持为空"
        );
    }
}
