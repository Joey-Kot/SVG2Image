fn main() {
    if let Err(error) = svg2image::run() {
        eprintln!("错误：{error:#}");
        std::process::exit(1);
    }
}
