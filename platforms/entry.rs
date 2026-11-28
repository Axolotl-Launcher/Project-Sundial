const IMAGES: axlauncher_frontend::WorkerImages = axlauncher_frontend::WorkerImages::new(
    include_bytes!(env!(
        "AXLAUNCHER_DOWNLOAD_IMAGE",
        "Build the desktop launcher with pnpm build or pnpm dev."
    )),
    include_bytes!(env!(
        "AXLAUNCHER_ACCOUNT_IMAGE",
        "Build the desktop launcher with pnpm build or pnpm dev."
    )),
    include_bytes!(env!(
        "AXLAUNCHER_JAVA_DISCOVERY_IMAGE",
        "Build the desktop launcher with pnpm build or pnpm dev."
    )),
    include_bytes!(env!(
        "AXLAUNCHER_INSTANCE_SETTINGS_IMAGE",
        "Build the desktop launcher with pnpm build or pnpm dev."
    )),
    include_bytes!(env!(
        "AXLAUNCHER_INSTALL_IMAGE",
        "Build the desktop launcher with pnpm build or pnpm dev."
    )),
);

pub(super) fn run() -> std::process::ExitCode {
    if let Some(status) = IMAGES.dispatch() {
        return status;
    }
    axlauncher_frontend::run(IMAGES);
    std::process::ExitCode::SUCCESS
}
