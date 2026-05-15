fn main() {
    #[cfg(windows)]
    {
        use std::path::Path;

        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let ico_path = Path::new(&manifest_dir).join("x-env.ico");

        if ico_path.exists() {
            match winres::WindowsResource::new()
                .set_icon_with_id(ico_path.to_str().unwrap(), "MAINICON")
                .compile()
            {
                Ok(_) => println!("Icon added successfully"),
                Err(e) => println!("Failed to add icon: {}", e),
            }
        }

        println!("cargo:rerun-if-changed={}", ico_path.display());
    }
}
