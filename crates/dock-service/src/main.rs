use orbcue_ipc::{default_endpoint, default_state_path};
use orbcue_service::spawn_persistent;
use std::sync::mpsc;

fn main() {
    let endpoint = default_endpoint();
    let state_path = default_state_path();
    let service = spawn_persistent(&endpoint, &state_path).unwrap_or_else(|error| {
        eprintln!("orbd: {error}");
        std::process::exit(1);
    });
    let (stop_tx, stop_rx) = mpsc::sync_channel(1);
    ctrlc::set_handler(move || {
        let _ = stop_tx.try_send(());
    })
    .unwrap_or_else(|error| {
        eprintln!("orbd: cannot install signal handler: {error}");
        std::process::exit(1);
    });
    println!("orbd ready on {}", endpoint.display());
    let _ = stop_rx.recv();
    service.shutdown();
}
