use clap::{Parser, Subcommand};
use std::path::PathBuf;
use terminal_poker::trainer::{
    replay::{Archive, Bookmark},
    storage::Store,
};

#[derive(Parser)]
#[command(about = "Browse completed OpenFelt hands and bookmark decisions")]
struct Args {
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    List,
    Show {
        hand: String,
        #[arg(long, default_value_t = 0)]
        decision: usize,
    },
    Outcome {
        hand: String,
    },
    Bookmark {
        hand: String,
        decision: usize,
    },
    Bookmarks,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let store = match args.data_dir {
        Some(root) => Store { root },
        None => Store::default_location()?,
    };
    let mut archive = Archive::load(&store.root)?;
    match args.command {
        Command::List => {
            for hand in &archive.hands {
                println!("{}\t{} decision(s)", hand.hand_id, hand.decisions.len());
            }
            if archive.skipped_records > 0 {
                eprintln!(
                    "Skipped {} invalid or unsupported record(s)",
                    archive.skipped_records
                );
            }
        }
        Command::Show { hand, decision } => {
            let item = archive.resolve(&hand, decision)?;
            println!(
                "Hand {hand} · decision {decision}\nStreet: {:?}\nCards: {}\nBoard: {}\nPot: {} · wager: {}\nStacks: {}\nPublic action history: {}\nLegal: {}\nChosen: {}\nFeedback: {}\n\nThis view is the saved pre-decision state. Use `outcome` for the separately stored final state.",
                item.decision.observation.phase,
                cards(&item.decision.observation.hole_cards),
                cards(&item.decision.observation.board),
                item.decision.observation.pot,
                item.decision.observation.wager,
                serde_json::to_string(&item.decision.observation.seats)?,
                serde_json::to_string(&item.decision.observation.history)?,
                serde_json::to_string(&item.decision.observation.legal)?,
                item.decision.accepted_action.description(),
                item.feedback.explanation,
            );
        }
        Command::Outcome { hand } => {
            let item = archive
                .hands
                .iter()
                .find(|item| item.hand_id == hand)
                .ok_or("Hand not found")?;
            println!(
                "Hand {hand} · final outcome (separate from coaching input)\n{}",
                serde_json::to_string_pretty(&item.outcome)?
            );
        }
        Command::Bookmark { hand, decision } => {
            archive.resolve(&hand, decision)?;
            archive.bookmarks.insert(Bookmark {
                hand_id: hand,
                decision,
            });
            store.save("bookmarks.json", &archive.bookmarks)?;
            println!("Bookmark saved");
        }
        Command::Bookmarks => {
            for bookmark in &archive.bookmarks {
                println!("{}\tdecision {}", bookmark.hand_id, bookmark.decision);
            }
        }
    }
    Ok(())
}

fn cards(cards: &[terminal_poker::game::deck::Card]) -> String {
    cards
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}
