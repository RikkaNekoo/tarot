//! 独立 CLI 读卡模式。
//!
//! 用法：
//!   cli list                 列出读卡器
//!   cli read [--reader NAME]  读取一次并打印原始十六进制数据
//!   cli monitor               轮询等待插卡后自动读取

use clap::{Parser, Subcommand};
use std::{thread, time::Duration};
use tarot_backend::{
    list_readers, read_from_reader, read_traveldoc_from_reader, reader::PcscManager,
};
use tarot_core::{Error, PassportKey};

#[derive(Parser)]
#[command(name = "tarot-cli", about = "PC/SC 智能卡原始数据读取工具")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 列出所有读卡器
    List,
    /// 读取一次
    Read {
        /// 指定读卡器名称（默认第一个）
        #[arg(long)]
        reader: Option<String>,
    },
    /// 持续监控，插卡即读
    Monitor {
        /// 指定读卡器名称（默认第一个）
        #[arg(long)]
        reader: Option<String>,
    },
    /// 读取旅行证件（电子护照 / 往来港澳通行证，需 MRZ 三要素）
    Traveldoc {
        /// 证件号
        #[arg(long)]
        doc_number: String,
        /// 出生日期 YYMMDD
        #[arg(long)]
        dob: String,
        /// 有效期 YYMMDD
        #[arg(long)]
        doe: String,
        /// 指定读卡器名称（默认第一个）
        #[arg(long)]
        reader: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Command::List => do_list(),
        Command::Read { reader } => do_read(reader),
        Command::Monitor { reader } => do_monitor(reader),
        Command::Traveldoc {
            doc_number,
            dob,
            doe,
            reader,
        } => do_traveldoc(doc_number, dob, doe, reader),
    };
    if let Err(e) = result {
        eprintln!("错误: {e}");
        std::process::exit(1);
    }
}

fn do_list() -> Result<(), Error> {
    let readers = list_readers()?;
    if readers.is_empty() {
        println!("未检测到读卡器");
    } else {
        println!("读卡器列表:");
        for (i, r) in readers.iter().enumerate() {
            println!("  [{i}] {r}");
        }
    }
    Ok(())
}

fn do_read(reader: Option<String>) -> Result<(), Error> {
    let mgr = PcscManager::new()?;
    let reader = match reader {
        Some(r) => r,
        None => mgr.first_reader()?,
    };
    println!("使用读卡器: {reader}");
    let data = read_from_reader(&mgr, &reader)?;
    print_result(&data);
    Ok(())
}

fn do_monitor(reader: Option<String>) -> Result<(), Error> {
    let mgr = PcscManager::new()?;
    let reader = match reader {
        Some(r) => r,
        None => mgr.first_reader()?,
    };
    println!("监控读卡器: {reader}（按 Ctrl-C 退出）");
    loop {
        match read_from_reader(&mgr, &reader) {
            Ok(data) => {
                print_result(&data);
                println!("\n请移开卡片后放入下一张...");
                // 等待卡片移开
                while matches!(
                    mgr.status(&reader),
                    Ok(tarot_backend::reader::CardStatus::Present)
                ) {
                    thread::sleep(Duration::from_millis(300));
                }
            }
            Err(Error::NoCard) => thread::sleep(Duration::from_millis(300)),
            Err(e) => {
                eprintln!("读取错误: {e}");
                thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

/// 读取旅行证件（护照 / 通行证，读取流程一致，类别由前端解析判定）。
fn do_traveldoc(
    doc_number: String,
    dob: String,
    doe: String,
    reader: Option<String>,
) -> Result<(), Error> {
    let key = PassportKey::new(doc_number, dob, doe);
    key.validate().map_err(Error::Passport)?;
    let mgr = PcscManager::new()?;
    let reader = match reader {
        Some(r) => r,
        None => mgr.first_reader()?,
    };
    println!("使用读卡器: {reader}");
    println!("MRZ 密钥: {}", key.mrz_key());
    let data = read_traveldoc_from_reader(&mgr, &reader, &key)?;
    print_result(&data);
    Ok(())
}

/// 打印读卡结果的原始十六进制数据与 APDU 历史。
fn print_result(data: &tarot_core::RawCardData) {
    println!("\n===== 读卡结果 =====");
    println!("卡类型: {}", data.card_type);
    println!("ATR: {}", data.atr);
    if !data.sub_cards.is_empty() {
        println!("子卡: {}", data.sub_cards.join(", "));
    }
    println!("\n--- 原始字段 ---");
    for (k, v) in &data.raw_fields {
        println!("  {k}: {v}");
    }
    println!("\n--- APDU 追踪 ({} 条) ---", data.apdu_history.len());
    for t in &data.apdu_history {
        println!("  >> {}", t.tx);
        println!("  << {}", t.rx);
    }
}
