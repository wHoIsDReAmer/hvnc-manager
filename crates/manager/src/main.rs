mod network;

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Result;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use network::{NetworkCommand, NetworkEvent, NetworkManager};

slint::include_modules!();

fn main() -> Result<()> {
    init_tracing();

    let ui = MainWindow::new()?;
    let ui_weak = ui.as_weak();

    let (cmd_tx, cmd_rx) = mpsc::channel::<NetworkCommand>(32);
    let (event_tx, event_rx) = mpsc::channel::<NetworkEvent>(32);

    let cmd_tx = Rc::new(RefCell::new(Some(cmd_tx)));
    let event_rx = Rc::new(RefCell::new(Some(event_rx)));

    // Connect callback
    let cmd_tx_clone = cmd_tx.clone();
    let ui_weak_clone = ui_weak.clone();
    ui.on_connect_to_relay(move || {
        if let Some(ui) = ui_weak_clone.upgrade() {
            let addr = ui.get_relay_addr().to_string();
            let token = ui.get_auth_token().to_string();
            if let Some(tx) = cmd_tx_clone.borrow().as_ref() {
                let _ = tx.try_send(NetworkCommand::Connect { addr, token });
            }
        }
    });

    // Disconnect callback
    let cmd_tx_clone = cmd_tx.clone();
    ui.on_disconnect_from_relay(move || {
        if let Some(tx) = cmd_tx_clone.borrow().as_ref() {
            let _ = tx.try_send(NetworkCommand::Disconnect);
        }
    });

    // Connect to client callback
    let cmd_tx_clone = cmd_tx.clone();
    ui.on_connect_to_client(move |client_id| {
        if let Some(tx) = cmd_tx_clone.borrow().as_ref() {
            let _ = tx.try_send(NetworkCommand::ConnectToClient {
                client_id: client_id as u64,
            });
        }
    });

    // Disconnect from client callback
    let cmd_tx_clone = cmd_tx.clone();
    ui.on_disconnect_from_client(move || {
        if let Some(tx) = cmd_tx_clone.borrow().as_ref() {
            let _ = tx.try_send(NetworkCommand::DisconnectFromClient);
        }
    });

    // Mouse move callback
    let cmd_tx_clone = cmd_tx.clone();
    ui.on_on_mouse_move(move |x, y| {
        if let Some(tx) = cmd_tx_clone.borrow().as_ref() {
            let _ = tx.try_send(NetworkCommand::MouseMove {
                x: x as i32,
                y: y as i32,
            });
        }
    });

    // Mouse click callback
    let cmd_tx_clone = cmd_tx.clone();
    ui.on_on_mouse_click(move |button, down| {
        if let Some(tx) = cmd_tx_clone.borrow().as_ref() {
            let _ = tx.try_send(NetworkCommand::MouseClick {
                button: button as u8,
                down,
            });
        }
    });

    // Key event callback
    let cmd_tx_clone = cmd_tx.clone();
    ui.on_on_key_event(move |key, down| {
        if let Some(tx) = cmd_tx_clone.borrow().as_ref() {
            let _ = tx.try_send(NetworkCommand::KeyEvent {
                key: key.to_string(),
                down,
            });
        }
    });

    // Start network thread
    let cmd_rx = cmd_rx;
    let event_tx = event_tx;
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut manager = NetworkManager::new(cmd_rx, event_tx);
            manager.run().await;
        });
    });

    // Event polling timer
    let ui_weak_clone = ui_weak.clone();
    let event_rx_clone = event_rx.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(16),
        move || {
            if let Some(ui) = ui_weak_clone.upgrade()
                && let Some(rx) = event_rx_clone.borrow_mut().as_mut()
            {
                while let Ok(event) = rx.try_recv() {
                    handle_network_event(&ui, event);
                }
            }
        },
    );

    ui.run()?;
    Ok(())
}

fn handle_network_event(ui: &MainWindow, event: NetworkEvent) {
    match event {
        NetworkEvent::Connected => {
            ui.set_connected(true);
            ui.set_status_text("Connected to relay".into());
            info!("Connected to relay");
        }
        NetworkEvent::Disconnected => {
            ui.set_connected(false);
            ui.set_in_session(false);
            ui.set_status_text("Disconnected".into());
            ui.set_clients(slint::ModelRc::default());
            info!("Disconnected from relay");
        }
        NetworkEvent::ClientListUpdated(clients) => {
            let model: Vec<ClientEntry> = clients
                .into_iter()
                .map(|c| ClientEntry {
                    client_id: c.client_id as i32,
                    node_name: c.node_name.into(),
                    is_busy: c.is_busy,
                })
                .collect();
            ui.set_clients(slint::ModelRc::new(slint::VecModel::from(model)));
        }
        NetworkEvent::SessionStarted {
            client_id,
            peer_name,
        } => {
            ui.set_in_session(true);
            ui.set_selected_client_id(client_id as i32);
            ui.set_peer_name(peer_name.clone().into());
            ui.set_status_text(format!("Session: {}", peer_name).into());
            info!("Session started with {}", peer_name);
        }
        NetworkEvent::SessionEnded { reason } => {
            ui.set_in_session(false);
            ui.set_selected_client_id(-1);
            ui.set_status_text(format!("Session ended: {}", reason).into());
            info!("Session ended: {}", reason);
        }
        NetworkEvent::FrameReceived {
            width,
            height,
            data,
        } => {
            let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&data, width, height);
            ui.set_desktop_image(Image::from_rgba8(buffer));
        }
        NetworkEvent::Error(msg) => {
            ui.set_status_text(format!("Error: {}", msg).into());
        }
    }
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .try_init();
    info!("manager starting");
}
