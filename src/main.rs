use panopticonbot::{run_bot, Settings, Scheduler};
use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() {
    // Logging
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::new()
    ).expect("Failed to set logger");
    
    // For graceful shutdown
    let mut terminate = signal(SignalKind::terminate()).expect("Failed to register signal");
    let mut hangup = signal(SignalKind::hangup()).expect("Failed to register signal");
    let mut quit = signal(SignalKind::quit()).expect("Failed to register signal");
    
    let signal = async {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
            _ = hangup.recv() => {}
            _ = quit.recv() => {}
        }
    };
    let settings = Settings::from_env(".env").expect("Failed to load configuration");
    let scheduler = Scheduler::new(std::time::Duration::from_secs(60));
    let bot = async {
        if let Err(e) = run_bot(settings, scheduler.clone()).await {
            tracing::error!("{:?}", e);
        }
    };
    tokio::select! {
        _ = bot => graceful_shutdown(scheduler).await,
        _ = signal => graceful_shutdown(scheduler).await,
    }
}

async fn graceful_shutdown(mut scheduler: Scheduler) {
    println!("\nGraceful shutdown... ");
    scheduler.complete_all().await;
}
