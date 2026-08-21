//! ビルド時に libmpv-2.dll を成果物ディレクトリへコピーする
//!
//! libs/ に配置した libmpv-2.dll を、生成された実行ファイルの隣へ自動で
//! コピーして、配布時の手間を減らす。

use std::path::Path;

fn main() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR がありません");
    // OUT_DIR = <target>/<profile>/build/<pkg>-<hash>/out
    // 3 つ親へ上がると <target>/<profile> になる
    let target_profile_dir = Path::new(&out_dir)
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();

    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR がありません");
    let src = Path::new(&manifest).join("libs").join("libmpv-2.dll");

    if src.is_file() {
        let dest = target_profile_dir.join("libmpv-2.dll");
        let _ = std::fs::copy(&src, &dest);
        println!("cargo:rerun-if-changed=libs/libmpv-2.dll");
    }
}
