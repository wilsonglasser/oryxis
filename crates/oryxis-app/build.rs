fn main() {
    // Embed the app icon into the Windows executable
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../resources/logo.ico");
        res.compile().expect("Failed to compile Windows resources");
    }
}
