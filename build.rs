fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("floatdo.ico");
    res.set("FileDescription", "FloatDo 待办");
    res.set("ProductName", "FloatDo");
    res.set("OriginalFilename", "floatdo_native.exe");
    if let Err(e) = res.compile() {
        eprintln!("winres error: {}", e);
    }
}
