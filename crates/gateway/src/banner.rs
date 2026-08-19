// Copyright 2026 Huy Nguyen Nhu
// SPDX-License-Identifier: Apache-2.0

use owo_colors::{OwoColorize, Rgb};
use terminal_size::{terminal_size, Width};

const ORANGE: Rgb = Rgb(0xEA, 0x58, 0x0C);

pub fn print_banner(cfg: &cram_vertex::Config, port: u16, update_notice: Option<(String, String)>) {
    let version = env!("CARGO_PKG_VERSION");
    let url = format!("http://127.0.0.1:{port}");
    let project = cfg.project();
    let location = cfg.location();

    let is_narrow = terminal_size().map(|(Width(w), _)| w < 80).unwrap_or(true);

    if is_narrow {
        println!("cram {} · vertex · {} · {}", version, location, url);
        if let Some((ver, rel_url)) = update_notice {
            println!("  update     {} available → {}", ver, rel_url);
        }
        return;
    }

    let box_top = format!("╭{}╮", "─".repeat(9));
    let box_mid = format!("│  {} │", "c r a m".color(ORANGE).bold());
    let box_bot = format!("╰{}╯", "─".repeat(9));

    let project_display = format!("{} · {}", project, location);

    println!();
    println!("   {}  {}", box_top.color(ORANGE), version);
    println!("   {}", box_mid);
    println!("   {}  editor ──→ [cram] ──→ vertex", box_bot.color(ORANGE));
    println!();
    println!("  {:>9}  {}", "gateway".bold(), url);
    println!("  {:>9}  {}/_cram/", "dashboard".bold(), url);
    println!("  {:>9}  {}", "upstream".bold(), project_display);
    if let Some((ver, rel_url)) = update_notice {
        println!("  {:>9}  {} available → {}", "update".bold(), ver, rel_url);
    }
    println!();
    println!("  waiting for requests… (ctrl-c to stop)");
    println!();
}
