use std::sync::Arc;
use swayipc::{Connection, Event, EventType, NodeLayout, WindowChange};
use tokio::sync::mpsc;
use tokio::task;

use clap::Parser;

/// Switches the layout of the focused window based on its dimensions.
async fn switch_splitting(conn: &mut Connection, allowed_workspaces: &[i32]) -> Result<(), String> {
    // Skip if the focused workspace is not in the allowed list (or allow all if the list is empty)
    if !allowed_workspaces.is_empty() {
        let focused_workspace = conn
            .get_workspaces()
            .map_err(|_| "get_workspaces() failed")?
            .iter()
            .find(|w| w.focused)
            .map(|w| w.num)
            .ok_or("Could not find the focused workspace")?;

        if !allowed_workspaces.contains(&focused_workspace) {
            return Ok(());
        }
    }

    let tree = conn.get_tree().map_err(|_| "get_tree() failed")?;
    let focused_node = tree
        .find_focused_as_ref(|n| n.focused)
        .ok_or("Could not find the focused node")?;

    // Skip if the focused node is floating, fullscreen, stacked, or tabbed
    if focused_node.node_type == swayipc::NodeType::FloatingCon
        || focused_node.percent.unwrap_or(1.0) > 1.0
        || focused_node.layout == NodeLayout::Stacked
        || focused_node.layout == NodeLayout::Tabbed
    {
        return Ok(());
    }

    let parent = tree
        .find_focused_as_ref(|n| n.nodes.iter().any(|n| n.focused))
        .ok_or("Could not find the parent node")?;

    let new_layout = if focused_node.rect.height > focused_node.rect.width {
        NodeLayout::SplitV
    } else {
        NodeLayout::SplitH
    };

    // Skip if the parent node already has the desired layout
    if new_layout == parent.layout {
        return Ok(());
    }

    let cmd = match new_layout {
        NodeLayout::SplitV => "splitv",
        NodeLayout::SplitH => "splith",
        _ => unreachable!(),
    };

    conn.run_command(cmd).map_err(|e| format!("run_command() failed: {}", e))?;
    Ok(())
}

#[derive(Parser)]
#[clap(version, author, about)]
struct Cli {
    /// Activate autotiling only on these workspaces
    #[clap(long, short = 'w')]
    workspace: Vec<i32>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Cli::parse();
    let (tx, mut rx) = mpsc::channel(100);
    let tx = Arc::new(tx);
    let workspaces = Arc::new(args.workspace);

    task::spawn(async move {
        let conn = Connection::new()?;
        let mut event_stream = conn.subscribe(&[EventType::Window])?;

        while let Some(event) = event_stream.next() {
            if let Ok(Event::Window(window_event)) = event {
                if let WindowChange::Focus = window_event.change {
                    let tx = Arc::clone(&tx);
                    let workspaces = Arc::clone(&workspaces);
                    task::spawn(async move {
                        let mut conn = Connection::new()?;
                        if let Err(err) = switch_splitting(&mut conn, &*workspaces).await {
                            eprintln!("Error: {}", err);
                        }
                        tx.send(()).await?;
                        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                    });
                }
            }
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });

    while let Some(_) = rx.recv().await {}

    Ok(())
}

