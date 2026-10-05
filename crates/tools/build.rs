fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=../../assets/ptools.ico");
    winresource::WindowsResource::new()
        .set("ProductName", "ptools")
        .set("FileDescription", "ptools native tools")
        .set_icon("../../assets/ptools.ico")
        .set_manifest(include_str!("app.manifest"))
        .compile()
        .expect("compile native tool resources");
}
