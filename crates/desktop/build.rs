fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    winresource::WindowsResource::new()
        .set("ProductName", "ptools")
        .set("FileDescription", "ptools native application launcher")
        .set_manifest(include_str!("app.manifest"))
        .compile()
        .expect("compile Windows manifest");
}
