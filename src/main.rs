use eframe::egui;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;

const TERMINAL_BG: egui::Color32 = egui::Color32::from_rgb(13, 17, 23);
const TERMINAL_FG: egui::Color32 = egui::Color32::from_rgb(171, 225, 171);
const BTN_GREEN: egui::Color32 = egui::Color32::from_rgb(35, 134, 54);
const BTN_MUTED: egui::Color32 = egui::Color32::from_rgb(40, 44, 52);
const BORDER: egui::Color32 = egui::Color32::from_rgb(48, 54, 61);
const LABEL_COL_W: f32 = 52.0;
const REFRESH_BTN_W: f32 = 34.0;

// Sentinel echoed by bash after each command; never appears in normal output.
const EXIT_MARKER: &str = "__SDFORMAT_EXIT_";
// Sent by our reader thread when the shell's stdout closes (shell died).
const SHELL_DIED: &str = "__SDFORMAT_SHELL_DIED__";

struct RootShell {
    stdin: std::process::ChildStdin,
    child: std::process::Child,
}

impl RootShell {
    fn spawn(tx: mpsc::Sender<String>) -> Result<Self, String> {
        let mut child = Command::new("pkexec")
            .args(["bash", "--norc", "--noprofile"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn pkexec bash: {e}"))?;

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let stdin = child.stdin.take().unwrap();

        let tx_out = tx.clone();
        std::thread::spawn(move || {
            BufReader::new(stdout)
                .lines()
                .for_each(|l| { tx_out.send(l.unwrap_or_default()).ok(); });
            // stdout closed = shell exited (normally or killed)
            tx_out.send(SHELL_DIED.to_string()).ok();
        });

        std::thread::spawn(move || {
            BufReader::new(stderr)
                .lines()
                .for_each(|l| { tx.send(l.unwrap_or_default()).ok(); });
        });

        Ok(RootShell { stdin, child })
    }

    fn run(&mut self, cmd: &str) -> std::io::Result<()> {
        writeln!(self.stdin, "{cmd}; echo \"{EXIT_MARKER}$?__\"")?;
        self.stdin.flush()
    }
}

impl Drop for RootShell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct App {
    devices: Vec<String>,
    selected: usize,
    label: String,
    log: String,
    running: bool,
    shell: Option<RootShell>,
    rx: Option<mpsc::Receiver<String>>,
}

impl App {
    fn new() -> Self {
        App {
            devices: list_devices(),
            selected: 0,
            label: String::new(),
            log: String::new(),
            running: false,
            shell: None,
            rx: None,
        }
    }

    fn refresh_devices(&mut self) {
        let current = self.devices.get(self.selected).cloned();
        self.devices = list_devices();
        if let Some(prev) = current {
            self.selected = self.devices.iter().position(|d| *d == prev).unwrap_or(0);
        }
    }

    fn run_format(&mut self) {
        if self.devices.is_empty() {
            self.log.push_str("ERROR: no device selected\n");
            return;
        }

        let binary = match which::which("sdFormatLinux") {
            Ok(p) => p,
            Err(_) => {
                self.log.push_str("ERROR: sdFormatLinux not found in PATH\n");
                return;
            }
        };

        let device = self.devices[self.selected]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();

        if device.is_empty() {
            self.log.push_str("ERROR: could not parse device path\n");
            return;
        }

        let label = self.label.trim().to_string();
        let binary_str = binary.to_string_lossy().to_string();

        let mut cmd = format!("{} -f", sh_quote(&binary_str));
        if !label.is_empty() {
            cmd.push_str(&format!(" -l {}", sh_quote(&label)));
        }
        cmd.push_str(&format!(" {}", sh_quote(&device)));

        self.log.push_str(&format!("$ {cmd}\n"));

        // Spawn root shell on first use; reuse on subsequent formats.
        if self.shell.is_none() {
            let (tx, rx) = mpsc::channel::<String>();
            self.rx = Some(rx);
            self.log.push_str("Requesting root (pkexec)…\n");
            match RootShell::spawn(tx) {
                Ok(shell) => {
                    self.shell = Some(shell);
                    self.log.push_str("Root shell ready. Will reuse for this session.\n");
                }
                Err(e) => {
                    self.log.push_str(&format!("ERROR: {e}\n"));
                    self.rx = None;
                    return;
                }
            }
        }

        let shell = self.shell.as_mut().unwrap();
        if let Err(e) = shell.run(&cmd) {
            self.log.push_str(&format!(
                "ERROR: shell write failed: {e}\nShell may have died — will re-auth on next format.\n"
            ));
            self.shell = None;
            return;
        }

        self.running = true;
    }
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn list_devices() -> Vec<String> {
    let Ok(out) = Command::new("lsblk")
        .args(["-d", "-n", "-o", "NAME,SIZE,MODEL", "-e", "7"])
        .output()
    else {
        return vec![];
    };

    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            let name = format!("/dev/{}", parts[0]);
            let rest = parts[1..].join("  ");
            format!("{name}  {rest}")
        })
        .collect()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Drain output channel every frame — collect first to avoid borrow conflict.
        let lines: Vec<String> = self.rx.as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();

        let mut cmd_finished = false;
        for line in lines {
            if line.starts_with(EXIT_MARKER) {
                let code = line
                    .strip_prefix(EXIT_MARKER)
                    .and_then(|s| s.strip_suffix("__"))
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(-1);
                self.log.push_str(&format!("Exit: {code}\n"));
                cmd_finished = true;
            } else if line == SHELL_DIED {
                self.log.push_str("Root shell exited. Will re-auth on next format.\n");
                self.shell = None;
                self.rx = None;
                cmd_finished = true;
            } else {
                self.log.push_str(&line);
                self.log.push('\n');
            }
        }
        if cmd_finished {
            self.running = false;
            // rx kept alive when shell still running — reused next format.
        }
        if self.running {
            ui.ctx().request_repaint();
        }

        // Format button anchored to bottom
        egui::TopBottomPanel::bottom("btn_panel")
            .frame(
                egui::Frame::default()
                    .inner_margin(egui::Margin::symmetric(20, 12))
                    .stroke(egui::Stroke::new(1.0, BORDER)),
            )
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let (btn_text, btn_fill) = if self.running {
                        ("Formatting…", BTN_MUTED)
                    } else {
                        ("Format SD Card", BTN_GREEN)
                    };
                    let btn = egui::Button::new(
                        egui::RichText::new(btn_text).size(15.0).strong(),
                    )
                    .min_size(egui::vec2(200.0, 42.0))
                    .fill(btn_fill)
                    .corner_radius(6.0);
                    ui.add_enabled(!self.running, btn).clicked().then(|| {
                        self.run_format();
                    });
                });
            });

        // Main content fills remaining space
        egui::CentralPanel::default()
            .frame(egui::Frame::default().inner_margin(egui::Margin::same(20)))
            .show_inside(ui, |ui| {
                // Header
                ui.label(
                    egui::RichText::new("SD Card Formatter")
                        .size(22.0)
                        .strong()
                        .color(egui::Color32::from_rgb(230, 237, 243)),
                );
                ui.add_space(3.0);
                ui.label(
                    egui::RichText::new("Forces FAT32 via sdFormatLinux")
                        .size(12.0)
                        .color(egui::Color32::from_rgb(110, 118, 129)),
                );
                ui.add_space(14.0);

                let label_color = egui::Color32::from_rgb(201, 209, 217);
                let gap = ui.spacing().item_spacing.x;

                // Device row
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [LABEL_COL_W, 20.0],
                        egui::Label::new(egui::RichText::new("Device").strong().color(label_color)),
                    );
                    let combo_w = ui.available_width() - REFRESH_BTN_W - gap;
                    let selected_text = self.devices.get(self.selected)
                        .cloned()
                        .unwrap_or_else(|| "(no devices found)".to_string());
                    egui::ComboBox::from_id_salt("device_combo")
                        .selected_text(selected_text)
                        .width(combo_w)
                        .show_ui(ui, |ui| {
                            for (i, dev) in self.devices.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.selected,
                                    i,
                                    egui::RichText::new(dev).font(egui::FontId::monospace(13.0)),
                                );
                            }
                        });
                    if ui.button("↺").on_hover_text("Refresh devices").clicked() {
                        self.refresh_devices();
                    }
                });

                ui.add_space(8.0);

                // Label row
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [LABEL_COL_W, 20.0],
                        egui::Label::new(egui::RichText::new("Label").strong().color(label_color)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.label)
                            .desired_width(f32::INFINITY)
                            .hint_text("optional volume label"),
                    );
                });

                ui.add_space(14.0);

                // Log area fills all remaining height.
                // inner_margin(10) = 20px total; subtract so ScrollArea fills exactly.
                let inner_h = (ui.available_height() - 20.0).max(40.0);
                egui::Frame::default()
                    .fill(TERMINAL_BG)
                    .inner_margin(egui::Margin::same(10))
                    .corner_radius(6.0)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .show(ui, |ui| {
                        ui.style_mut().visuals.override_text_color = Some(TERMINAL_FG);
                        egui::ScrollArea::vertical()
                            .min_scrolled_height(inner_h)
                            .max_height(inner_h)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.log)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::FontId::monospace(13.0))
                                        .interactive(false),
                                );
                            });
                    });
            });
    }
}

fn setup_style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.visuals.window_corner_radius = egui::CornerRadius::same(8);
    ctx.set_global_style(style);
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("sdFormatLinux-Gui")
            .with_inner_size([540.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "sdFormatLinux-Gui",
        options,
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
    .unwrap();
}
