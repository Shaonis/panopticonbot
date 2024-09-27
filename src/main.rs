use panopticonbot::{run_bot, Settings, Scheduler};
use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logging
    tracing::subscriber::set_global_default(tracing_subscriber::FmtSubscriber::new())?;
    
    // For graceful shutdown
    let mut terminate = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    let mut quit = signal(SignalKind::quit())?;

    let signals = async {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
            _ = hangup.recv() => {}
            _ = quit.recv() => {}
        }
    };
    let settings = Settings::try_from(".env")?;
    let mut scheduler = Scheduler::new(std::time::Duration::from_secs(60));
    tokio::select! {
        _ = run_bot(settings, scheduler.clone()) => {},
        _ = signals => {
            println!("\nGraceful shutdown... ");
            scheduler.complete_all().await;
        },
    }
    
    Ok(())
}
