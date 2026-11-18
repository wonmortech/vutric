//! The compat adapter process `vutric-compat` (ADR-0054, ADR-0056, ADR-0062): `LocalSend`, the browser page,
//! Quick Share and KDE Connect. Started by the daemon through [`vutric_compat_localsend::host::Adapter`]; speaks
//! [`vutric_compat_localsend::ipc`] on stdin/stdout and exits when stdin closes. The loop itself is
//! [`vutric_compat_localsend::serve`], which the phone runs in its own process (ADR-0066).

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    vutric_compat_localsend::serve::serve(tokio::io::stdin(), tokio::io::stdout()).await
}
