use futures_lite::StreamExt;
use swayipc_async::{Connection, Event, EventType, NodeLayout, WindowChange, Node};
use tokio::sync::mpsc;
use tokio::task;
use std::sync::Arc;

use clap::Parser;

/// Switches the layout of the focused window based on its dimensions.
async fn switch_splitting(conn: &mut Connection, allowed_workspaces: &Arc<Vec<i32>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Skip if the focused workspace is not in the allowed list (or allow all if the list is empty)
    if !allowed_workspaces.is_empty() {
        let workspaces = conn.get_workspaces().await?;
        let focused_workspace = workspaces
            .iter()
            .find(|w| w.focused)
            .map(|w| w.num)
            .ok_or("Could not find the focused workspace")?;

        if !allowed_workspaces.contains(&focused_workspace) {
            return Ok(());
        }
    }

    let tree = conn.get_tree().await?;
    let (focused_node, parent) = find_nodes(&tree)?;

    // Skip if the focused node is floating, fullscreen, stacked, or tabbed
    if focused_node.node_type == swayipc_async::NodeType::FloatingCon
        || focused_node.percent.unwrap_or(1.0) > 1.0
        || focused_node.layout == NodeLayout::Stacked
        || focused_node.layout == NodeLayout::Tabbed
    {
        return Ok(());
    }

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

    conn.run_command(cmd).await?;
    Ok(())
}

fn find_nodes<'a>(node: &'a Node) -> Result<(&'a Node, &'a Node), Box<dyn std::error::Error + Send + Sync>> {
    let focused_node = node.find_focused_as_ref(&|n: &Node| n.focused)
        .ok_or("Could not find the focused node")?;
    let parent_node = node.find_focused_as_ref(&|n: &Node| n.nodes.iter().any(|n| n.focused))
        .ok_or("Could not find the parent node")?;
    Ok((focused_node, parent_node))
}

async fn create_conn_and_switch_splitting(tx: mpsc::Sender<()>, workspaces: Arc<Vec<i32>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut conn = Connection::new().await?;
    if let Err(err) = switch_splitting(&mut conn, &workspaces).await {
        eprintln!("Error: {}", err);
    }
    tx.send(()).await?;
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

    let workspaces = Arc::new(args.workspace); // Wrap the vector in an Arc here

    task::spawn(async move {
        let conn = Connection::new().await?;
        let mut event_stream = conn.subscribe(&[EventType::Window]).await?;

        while let Some(event) = event_stream.next().await {
            if let Ok(Event::Window(window_event)) = event {
                if let WindowChange::Focus = window_event.change {
                    let tx = tx.clone();
                    let workspaces_clone = Arc::clone(&workspaces);
                    task::spawn(async move {
                        if let Err(e) = create_conn_and_switch_splitting(tx, workspaces_clone).await {
                            eprintln!("Error occurred: {:?}", e);
                        }
                    });
                }
            }
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });

    while let Some(_) = rx.recv().await {}

    Ok(())
}

