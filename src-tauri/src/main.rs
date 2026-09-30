// Prevent startup errors on NVIDIA + Wayland (WebKitGTK DMABUF renderer bug).
fn apply_webkit_workarounds() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        let nvidia = std::path::Path::new("/proc/driver/nvidia/version").exists()
            || std::env::var_os("NVIDIA_DRIVER").is_some();
        if nvidia {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
}

fn main() {
    apply_webkit_workarounds();
    tilde_lib::run()
}
