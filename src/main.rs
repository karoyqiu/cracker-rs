use anyhow::Result;
use clap::Parser;
use tokio::{
  process::Command,
  sync::broadcast::{self, error::TryRecvError},
  task::JoinHandle,
};

// 命令行参数
#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
  /// The alphabet
  #[arg(short, long, default_value = "abcdefghijklmnopqrstuvwxyz0123456789")]
  alphabet: String,

  /// The max length of the password
  #[arg(short, long, default_value_t = 8)]
  max_length: usize,

  /// The concurrency of cracking jobs. 0 means the number of logical CPU cores.
  #[arg(short, long, default_value_t = 0)]
  concurrency: usize,

  /// File to crack
  file: String,
}

fn should_stop(rx: &mut broadcast::Receiver<bool>) -> bool {
  if let Err(err) = rx.try_recv() {
    match err {
      TryRecvError::Empty => return false,
      _ => {}
    }
  }

  true
}

#[tokio::main]
async fn main() -> Result<()> {
  // 分析命令行参数
  let args = Args::parse();
  let concurrency = if args.concurrency == 0 {
    num_cpus::get()
  } else {
    args.concurrency
  };

  println!("Alphabet:   {}", args.alphabet);
  println!("Max length: {}", args.max_length);
  println!("Concurrency: {}", concurrency);
  println!("File:       {}", args.file);

  // 任务句柄
  let mut handles: Vec<JoinHandle<()>> = Vec::new();
  // 停止信号
  let (stop_sender, mut stop_receiver) = broadcast::channel::<bool>(1);
  // 密码队列
  let (sender, receiver) = async_channel::bounded::<String>(concurrency * 4);

  // 密码生成
  handles.push(tokio::spawn(async move {
    let a = allwords::Alphabet::from_chars_in_str(args.alphabet).unwrap();
    let mut words = a.all_words_with_len(args.max_length, Some(args.max_length));

    while let Some(word) = words.next() {
      let result = sender.send(word).await;

      if result.is_err() {
        println!("Failed to send password: {}", result.err().unwrap());
        break;
      }

      // 检查停止信号
      if should_stop(&mut stop_receiver) {
        break;
      }
    }
  }));

  // 破解
  for _ in 0..concurrency {
    let file = args.file.clone();
    let word_receiver = receiver.clone();
    let stop_sender = stop_sender.clone();
    let mut stop_receiver = stop_sender.subscribe();

    handles.push(tokio::spawn(async move {
      while let Ok(word) = word_receiver.recv().await {
        // unrar
        let status = Command::new("unrar")
          .arg("t")
          .arg(String::from("-p") + &word)
          .arg("-y")
          .arg("-inul")
          .arg(&file)
          .status()
          .await;

        if let Ok(status) = status {
          if let Some(code) = status.code() {
            if code == 0 {
              // Bingo!
              println!("Bingo! {}", &word);

              // 停止
              stop_sender.send(true).unwrap();
              break;
            } else {
              println!("Wrong: {} ({})", &word, code);
            }
          }
        }

        // 检查停止信号
        if should_stop(&mut stop_receiver) {
          break;
        }
      }
    }));
  }

  // 等待
  for ele in handles {
    ele.await?;
  }

  Ok(())
}
