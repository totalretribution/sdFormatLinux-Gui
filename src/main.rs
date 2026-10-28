mod update;

use eframe::egui;
use egui::text::{LayoutJob, TextWrapping};
use egui::{Color32, FontId, RichText, Stroke, TextFormat};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;

const ICON_PNG: &[u8] = include_bytes!("../assets/icons/hicolor/256x256/apps/sdformat-linux-gui.png");

fn load_icon() -> egui::IconData {
    let img = image::load_from_memory(ICON_PNG).unwrap().into_rgba8();
    let (w, h) = image::GenericImageView::dimensions(&img);
    egui::IconData { rgba: img.into_raw(), width: w, height: h }
}

// GitHub-dark palette. setup_style puts most of it into Visuals; the rest is
// used directly where egui has no slot for it.
const BG: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);
const SURFACE: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
const INSET: Color32 = Color32::from_rgb(0x01, 0x04, 0x09);
const CONTROL: Color32 = Color32::from_rgb(0x21, 0x26, 0x2d);
const BORDER: Color32 = Color32::from_rgb(0x30, 0x36, 0x3d);
const TEXT: Color32 = Color32::from_rgb(0xe6, 0xed, 0xf3);
const TEXT_LABEL: Color32 = Color32::from_rgb(0xc9, 0xd1, 0xd9);
const MUTED: Color32 = Color32::from_rgb(0x8b, 0x94, 0x9e);
const DIM: Color32 = Color32::from_rgb(0x6e, 0x76, 0x81);
const LINK: Color32 = Color32::from_rgb(0x58, 0xa6, 0xff);
const SELECT_BG: Color32 = Color32::from_rgb(0x0f, 0x2a, 0x44);
const SELECT_MUTED: Color32 = Color32::from_rgb(0xa5, 0xb3, 0xc2);
const GREEN: Color32 = Color32::from_rgb(0x23, 0x86, 0x36);
const TERMINAL_FG: Color32 = Color32::from_rgb(0xab, 0xe1, 0xab);
const INFO_TEXT: Color32 = Color32::from_rgb(0x79, 0xc0, 0xff);
const INFO_BORDER: Color32 = Color32::from_rgb(0x1f, 0x6f, 0xeb);
const AMBER: Color32 = Color32::from_rgb(0xd2, 0x99, 0x22);
const AMBER_BG: Color32 = Color32::from_rgb(0x2b, 0x21, 0x11);
const AMBER_BORDER: Color32 = Color32::from_rgb(0x5a, 0x43, 0x14);
const DANGER: Color32 = Color32::from_rgb(0xb6, 0x23, 0x24);
const DANGER_BG: Color32 = Color32::from_rgb(0x2d, 0x12, 0x14);
const DANGER_BORDER: Color32 = Color32::from_rgb(0x8e, 0x15, 0x19);
const DANGER_STROKE: Color32 = Color32::from_rgb(0xf8, 0x51, 0x49);
const DANGER_TEXT: Color32 = Color32::from_rgb(0xff, 0xdc, 0xd7);
const DANGER_MUTED: Color32 = Color32::from_rgb(0xf0, 0xb6, 0xb0);
const SUCCESS_BG: Color32 = Color32::from_rgb(0x0f, 0x2a, 0x1a);
const SUCCESS_BORDER: Color32 = Color32::from_rgb(0x2e, 0xa0, 0x43);
const SUCCESS_ICON: Color32 = Color32::from_rgb(0x3f, 0xb9, 0x50);
const SUCCESS_TEXT: Color32 = Color32::from_rgb(0xaf, 0xf5, 0xb4);
const SUCCESS_MUTED: Color32 = Color32::from_rgb(0x9f, 0xd8, 0xa5);
const SUCCESS_CHIP: Color32 = Color32::from_rgb(0x7e, 0xe7, 0x87);

const REFRESH_BTN_W: f32 = 36.0;
// Width of the device path column in the device popup.
const PATH_COL_W: f32 = 120.0;
// FAT32 volume labels hold at most 11 characters.
const LABEL_MAX: usize = 11;
// A disk with anything mounted here is the running system's; never offer it.
const SYSTEM_MOUNTS: &[&str] = &["/", "/boot", "/boot/efi", "/efi", "/home", "/usr", "/var", "[SWAP]"];

// Private copy shipped in the .deb/.rpm; source builds fall back to $PATH.
const BUNDLED_FORMATTER: &str = "/usr/libexec/sdformatlinux-gui/sdFormatLinux";

// Sentinel echoed by bash after each command; never appears in normal output.
const EXIT_MARKER: &str = "__SDFORMAT_EXIT_";
// Sent by our reader thread when the shell's stdout closes (shell died).
const SHELL_DIED: &str = "__SDFORMAT_SHELL_DIED__";

struct RootShell {
    stdin: Option<std::process::ChildStdin>,
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

        Ok(RootShell { stdin: Some(stdin), child })
    }

    fn run(&mut self, cmd: &str) -> std::io::Result<()> {
        let stdin = self.stdin.as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin closed"))?;
        writeln!(stdin, "{cmd}; echo \"{EXIT_MARKER}$?__\"")?;
        stdin.flush()
    }
}

impl Drop for RootShell {
    fn drop(&mut self) {
        // pkexec child runs as root — kill() fails with EPERM. Close stdin
        // to send EOF; bash exits cleanly. Poll try_wait with short timeout
        // so close-on-quit never blocks the UI thread.
        if let Some(mut stdin) = self.stdin.take() {
            let _ = writeln!(stdin, "exit");
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if std::time::Instant::now() >= deadline => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(_) => return,
            }
        }
    }
}

#[derive(Clone)]
struct Device {
    path: String,
    model: String,
    tran: String,
    size: u64,
    system: bool,
}

impl Device {
    /// Model and bus, e.g. "SanDisk Ultra · USB".
    fn detail(&self) -> String {
        let bus = match self.tran.as_str() {
            "usb" => "USB".to_string(),
            "mmc" => "SD card reader".to_string(),
            t => t.to_uppercase(),
        };
        match (self.model.as_str(), bus.as_str()) {
            ("", "") => "Unknown device".to_string(),
            (m, "") => m.to_string(),
            ("", b) => b.to_string(),
            (m, b) => format!("{m} · {b}"),
        }
    }

    fn size_label(&self) -> String {
        // Decimal units, so a "32 GB" card reads close to what is printed on it.
        let b = self.size as f64;
        if b >= 1e12 {
            format!("{:.1} TB", b / 1e12)
        } else if b >= 1e9 {
            format!("{:.1} GB", b / 1e9)
        } else {
            format!("{:.0} MB", b / 1e6)
        }
    }
}

/// What a format run was for, kept to describe its result.
#[derive(Clone)]
struct RunInfo {
    path: String,
    summary: String,
    label: String,
}

struct App {
    devices: Vec<Device>,
    selected: Option<usize>,
    label: String,
    log: String,
    running: bool,
    confirming: bool,
    run_info: Option<RunInfo>,
    outcome: Option<(RunInfo, i32)>,
    formatter: Option<PathBuf>,
    shell: Option<RootShell>,
    rx: Option<mpsc::Receiver<String>>,
    update_check: Option<mpsc::Receiver<String>>,
    new_version: Option<String>,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let devices = list_devices();
        App {
            selected: first_usable(&devices),
            devices,
            label: String::new(),
            log: String::new(),
            running: false,
            confirming: false,
            run_info: None,
            outcome: None,
            formatter: find_formatter(),
            shell: None,
            rx: None,
            update_check: update::spawn(ctx),
            new_version: None,
        }
    }

    fn poll_update(&mut self) {
        let Some(rx) = &self.update_check else {
            return;
        };
        match rx.try_recv() {
            Ok(v) => {
                self.new_version = Some(v);
                self.update_check = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => self.update_check = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    fn selected_device(&self) -> Option<&Device> {
        self.selected
            .and_then(|i| self.devices.get(i))
            .filter(|d| !d.system)
    }

    fn refresh_devices(&mut self) {
        let current = self.selected_device().map(|d| d.path.clone());
        self.devices = list_devices();
        self.selected = current
            .and_then(|p| self.devices.iter().position(|d| d.path == p && !d.system))
            .or_else(|| first_usable(&self.devices));
        self.formatter = find_formatter();
    }

    fn run_format(&mut self) {
        let Some(dev) = self.selected_device().cloned() else {
            self.log.push_str("ERROR: no device selected\n");
            return;
        };

        let Some(binary) = find_formatter() else {
            self.log.push_str("ERROR: sdFormatLinux not found (bundled or in PATH)\n");
            self.formatter = None;
            return;
        };

        let label = self.label.trim().to_string();
        let binary_str = binary.to_string_lossy().to_string();

        let mut cmd = format!("{} -f", sh_quote(&binary_str));
        if !label.is_empty() {
            cmd.push_str(&format!(" -l {}", sh_quote(&label)));
        }
        cmd.push_str(&format!(" {}", sh_quote(&dev.path)));

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

        self.run_info = Some(RunInfo {
            path: dev.path.clone(),
            summary: format!("{}, {}", dev.detail(), dev.size_label()),
            label,
        });
        self.outcome = None;
        self.running = true;
    }

    fn finish(&mut self, code: i32) {
        self.running = false;
        // rx kept alive when shell still running — reused next format.
        if let Some(info) = self.run_info.take() {
            self.outcome = Some((info, code));
        }
    }

    fn drain_output(&mut self) {
        // Collect first to avoid borrow conflict.
        let lines: Vec<String> = self.rx.as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();

        for line in lines {
            if let Some(rest) = line.strip_prefix(EXIT_MARKER) {
                let code = rest
                    .strip_suffix("__")
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(-1);
                self.log.push_str(&format!("Exit: {code}\n"));
                self.finish(code);
            } else if line == SHELL_DIED {
                self.log.push_str("Root shell exited. Will re-auth on next format.\n");
                self.shell = None;
                self.rx = None;
                self.finish(-1);
            } else {
                self.log.push_str(&line);
                self.log.push('\n');
            }
        }
    }

    fn header_ui(&self, ui: &mut egui::Ui) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("SD Card Formatter").size(22.0).color(TEXT));
                ui.label(RichText::new("Formats as FAT32 with sdFormatLinux").size(12.0).color(MUTED));
            });
            // Hidden while formatting so a stray click can't open a browser mid-run.
            if let (Some(v), false) = (&self.new_version, self.running) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.hyperlink_to(
                        RichText::new(format!("↑ {v} available")).size(12.0),
                        update::RELEASES_URL,
                    )
                    .on_hover_text(format!(
                        "You have v{}. Opens the release page.",
                        env!("CARGO_PKG_VERSION")
                    ));
                });
            }
        });
    }

    fn outcome_ui(&self, ui: &mut egui::Ui) {
        let Some((info, code)) = &self.outcome else {
            return;
        };
        let ok = *code == 0;
        let (fill, stroke, icon, icon_col, title_col, sub_col) = if ok {
            (SUCCESS_BG, SUCCESS_BORDER, "✔", SUCCESS_ICON, SUCCESS_TEXT, SUCCESS_MUTED)
        } else {
            (DANGER_BG, DANGER_BORDER, "⚠", DANGER_STROKE, DANGER_TEXT, DANGER_MUTED)
        };
        let title = if ok {
            format!("Formatted {} as FAT32", info.path)
        } else {
            format!("Formatting {} failed (exit {code})", info.path)
        };
        let sub = match (ok, info.label.is_empty()) {
            (true, true) => info.summary.clone(),
            (true, false) => format!("Label {} · {}", info.label, info.summary),
            (false, _) => "See the output below.".to_string(),
        };

        egui::Frame::default()
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, stroke))
            .corner_radius(6.0_f32)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new(icon).size(16.0).color(icon_col));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(title).size(14.0).color(title_col));
                        ui.label(RichText::new(sub).size(12.0).color(sub_col));
                    });
                });
            });
        ui.add_space(12.0);
    }

    fn device_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Device").size(13.0).color(TEXT_LABEL));
        ui.horizontal(|ui| {
            let combo_w = ui.available_width() - REFRESH_BTN_W - ui.spacing().item_spacing.x;
            let danger = self.confirming;
            let selected_text: egui::WidgetText = match self.selected_device() {
                Some(d) => device_job(d, danger).into(),
                None if self.devices.is_empty() => RichText::new("No devices found").color(MUTED).into(),
                None => RichText::new("No removable devices").color(MUTED).into(),
            };
            let devices = &self.devices;
            let selected = &mut self.selected;
            ui.scope(|ui| {
                // While the erase warning is up, mark the drive it is about.
                if danger {
                    let w = &mut ui.visuals_mut().widgets;
                    for state in [&mut w.inactive, &mut w.hovered, &mut w.open] {
                        state.weak_bg_fill = DANGER_BG;
                        state.bg_stroke = Stroke::new(1.0_f32, DANGER_STROKE);
                        state.fg_stroke.color = DANGER_MUTED;
                    }
                }
                egui::ComboBox::from_id_salt("device_combo")
                    .selected_text(selected_text)
                    .width(combo_w)
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .show_ui(ui, |ui| {
                        for (i, dev) in devices.iter().enumerate() {
                            if device_row(ui, dev, *selected == Some(i)).clicked() {
                                *selected = Some(i);
                            }
                        }
                    });
            });
            if ui
                .add_sized([REFRESH_BTN_W, 34.0], egui::Button::new("↺"))
                .on_hover_text("Refresh devices")
                .clicked()
            {
                self.refresh_devices();
            }
        });
    }

    fn label_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Volume label").size(13.0).color(TEXT_LABEL));
            ui.label(RichText::new("(optional)").size(13.0).color(MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let count = self.label.chars().count();
                ui.label(RichText::new(format!("{count} / {LABEL_MAX}")).size(12.0).color(MUTED));
            });
        });
        let edit = egui::TextEdit::singleline(&mut self.label)
            .char_limit(LABEL_MAX)
            .font(FontId::monospace(13.0))
            .margin(egui::Margin::symmetric(10, 8))
            .desired_width(f32::INFINITY)
            .hint_text("e.g. MYCARD");
        if ui.add(edit).changed() {
            // FAT32 labels are uppercase; show exactly what will be written.
            self.label = self.label.to_uppercase().chars().take(LABEL_MAX).collect();
        }
        ui.label(RichText::new("Up to 11 characters, saved in uppercase.").size(12.0).color(MUTED));
    }

    fn status_chip(&self, ui: &mut egui::Ui) {
        let (text, fg, fill, stroke) = if self.running {
            ("Formatting".to_string(), INFO_TEXT, SELECT_BG, INFO_BORDER)
        } else {
            match &self.outcome {
                Some((_, 0)) => ("Done · exit 0".to_string(), SUCCESS_CHIP, SUCCESS_BG, SUCCESS_BORDER),
                Some((_, code)) => (format!("Failed · exit {code}"), DANGER_MUTED, DANGER_BG, DANGER_BORDER),
                None => ("Ready".to_string(), TEXT_LABEL, CONTROL, BORDER),
            }
        };
        egui::Frame::default()
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, stroke))
            .corner_radius(10.0_f32)
            .inner_margin(egui::Margin::symmetric(8, 1))
            .show(ui, |ui| {
                // Parent layout is right-to-left: the label lands right of the spinner.
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new(text).size(11.0).color(fg));
                if self.running {
                    ui.add(egui::Spinner::new().size(10.0).color(fg));
                }
            });
    }

    fn output_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Output").size(13.0).color(TEXT_LABEL));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.log.is_empty() && !self.running && ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(self.log.clone());
                }
                self.status_chip(ui);
            });
        });
        ui.add_space(2.0);

        // inner_margin(10) = 20px total; subtract so the log fills exactly.
        let inner_h = (ui.available_height() - 20.0).max(40.0);
        egui::Frame::default()
            .fill(INSET)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(6.0_f32)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.log.is_empty() {
                    ui.set_min_height(inner_h);
                    ui.label(
                        RichText::new("Formatter output appears here.")
                            .font(FontId::monospace(12.0))
                            .color(MUTED),
                    );
                    return;
                }
                ui.style_mut().visuals.override_text_color = Some(TERMINAL_FG);
                egui::ScrollArea::vertical()
                    .min_scrolled_height(inner_h)
                    .max_height(inner_h)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.log)
                                .desired_width(f32::INFINITY)
                                .font(FontId::monospace(12.0))
                                .frame(egui::Frame::NONE)
                                .interactive(false),
                        );
                    });
            });
    }

    fn actions_ui(&mut self, ui: &mut egui::Ui) {
        let w = ui.available_width();
        if self.running {
            let path = self.run_info.as_ref().map(|r| r.path.as_str()).unwrap_or("card");
            running_bar(ui, w, &format!("Formatting {path}…"));
        } else if self.confirming {
            self.confirm_ui(ui);
        } else {
            let (text, enabled) = match self.selected_device() {
                Some(d) => (format!("Format {} · {}", d.path, d.size_label()), self.formatter.is_some()),
                None => ("Format SD Card".to_string(), false),
            };
            let btn = egui::Button::new(RichText::new(text).size(15.0).color(Color32::WHITE))
                .fill(GREEN)
                .corner_radius(6.0_f32)
                .min_size(egui::vec2(w, 44.0));
            if ui.add_enabled(enabled, btn).clicked() {
                self.confirming = true;
            }
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (text, color) = if self.running {
                ("Keep the card connected until this finishes.", MUTED)
            } else {
                match &self.formatter {
                    Some(p) if p == Path::new(BUNDLED_FORMATTER) => ("sdFormatLinux · bundled", MUTED),
                    Some(_) => ("sdFormatLinux · from PATH", MUTED),
                    None => ("sdFormatLinux not found", AMBER),
                }
            };
            ui.label(RichText::new(text).size(11.0).color(color));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(concat!("v", env!("CARGO_PKG_VERSION"))).size(11.0).color(MUTED));
            });
        });
    }

    fn confirm_ui(&mut self, ui: &mut egui::Ui) {
        let Some(dev) = self.selected_device().cloned() else {
            self.confirming = false;
            return;
        };
        let mut cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let mut erase = false;

        egui::Frame::default()
            .fill(DANGER_BG)
            .stroke(Stroke::new(1.0_f32, DANGER_BORDER))
            .corner_radius(6.0_f32)
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("⚠").size(16.0).color(DANGER_STROKE));
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!("Erase everything on {}?", dev.path))
                                .size(14.0)
                                .color(DANGER_TEXT),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{}, {}. All files on it are deleted and cannot be recovered.",
                                dev.detail(),
                                dev.size_label()
                            ))
                            .size(12.0)
                            .color(DANGER_MUTED),
                        );
                    });
                });
                ui.add_space(8.0);
                ui.columns(2, |cols| {
                    let bw = cols[0].available_width();
                    cancel |= cols[0]
                        .add(egui::Button::new(RichText::new("Cancel").size(14.0)).min_size(egui::vec2(bw, 40.0)))
                        .clicked();
                    erase = cols[1]
                        .add(
                            egui::Button::new(RichText::new("Erase and format").size(14.0).color(Color32::WHITE))
                                .fill(DANGER)
                                .min_size(egui::vec2(bw, 40.0)),
                        )
                        .clicked();
                });
            });

        if erase {
            self.confirming = false;
            self.run_format();
        } else if cancel {
            self.confirming = false;
        }
    }
}

/// Disabled-looking bar with a spinner, shown in place of the format button.
fn running_bar(ui: &mut egui::Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 44.0), egui::Sense::hover());
    let painter = ui.painter().clone();
    painter.rect(rect, 6.0_f32, CONTROL, Stroke::new(1.0_f32, BORDER), egui::StrokeKind::Inside);
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(15.0), MUTED);
    let x0 = rect.center().x - (16.0 + 10.0 + galley.size().x) / 2.0;
    let y = rect.center().y;
    ui.put(
        egui::Rect::from_min_size(egui::pos2(x0, y - 8.0), egui::vec2(16.0, 16.0)),
        egui::Spinner::new().size(16.0).color(MUTED),
    );
    painter.galley(egui::pos2(x0 + 26.0, y - galley.size().y / 2.0), galley, MUTED);
}

fn text_format(font_id: FontId, color: Color32) -> TextFormat {
    TextFormat { font_id, color, valign: egui::Align::Center, ..Default::default() }
}

/// The combo box's closed text: mono path, muted detail, size.
fn device_job(dev: &Device, danger: bool) -> LayoutJob {
    let (main, sub) = if danger { (DANGER_TEXT, DANGER_MUTED) } else { (TEXT, MUTED) };
    let mut job = LayoutJob::default();
    job.append(&dev.path, 0.0, text_format(FontId::monospace(13.0), main));
    job.append(&dev.detail(), 10.0, text_format(FontId::proportional(12.0), sub));
    job.append(&dev.size_label(), 10.0, text_format(FontId::proportional(13.0), main));
    job
}

/// One popup row: path, detail and size in columns. System disks are listed
/// but can't be picked.
fn device_row(ui: &mut egui::Ui, dev: &Device, selected: bool) -> egui::Response {
    let width = ui.available_width().max(360.0);
    let sense = if dev.system { egui::Sense::hover() } else { egui::Sense::click() };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 32.0), sense);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            !dev.system,
            selected,
            format!("{} {} {}", dev.path, dev.detail(), dev.size_label()),
        )
    });
    if !ui.is_rect_visible(rect) {
        return resp;
    }

    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 4.0_f32, SELECT_BG);
    } else if resp.hovered() && !dev.system {
        p.rect_filled(rect, 4.0_f32, CONTROL);
    }
    let (main, sub) = match (dev.system, selected) {
        (true, _) => (DIM, DIM),
        (false, true) => (TEXT, SELECT_MUTED),
        (false, false) => (TEXT, MUTED),
    };
    let y = rect.center().y;
    let right = rect.right() - 8.0;
    p.text(egui::pos2(rect.left() + 8.0, y), egui::Align2::LEFT_CENTER, &dev.path, FontId::monospace(13.0), main);

    let right_w = if dev.system {
        paint_tag(p, right, y, "System disk")
    } else {
        let g = p.layout_no_wrap(dev.size_label(), FontId::proportional(13.0), main);
        let w = g.size().x;
        p.galley(egui::pos2(right - w, y - g.size().y / 2.0), g, main);
        w
    };

    let detail_x = rect.left() + 8.0 + PATH_COL_W;
    let mut job = LayoutJob::single_section(dev.detail(), text_format(FontId::proportional(12.0), sub));
    job.wrap = TextWrapping::truncate_at_width((right - right_w - 12.0 - detail_x).max(0.0));
    let g = p.layout_job(job);
    p.galley(egui::pos2(detail_x, y - g.size().y / 2.0), g, sub);
    resp
}

/// Small amber pill ending at `right`; returns its width.
fn paint_tag(p: &egui::Painter, right: f32, y: f32, text: &str) -> f32 {
    let g = p.layout_no_wrap(text.to_string(), FontId::proportional(11.0), AMBER);
    let size = g.size() + egui::vec2(16.0, 4.0);
    let rect = egui::Rect::from_min_size(egui::pos2(right - size.x, y - size.y / 2.0), size);
    p.rect(rect, 10.0_f32, AMBER_BG, Stroke::new(1.0_f32, AMBER_BORDER), egui::StrokeKind::Inside);
    p.galley(rect.min + egui::vec2(8.0, 2.0), g, AMBER);
    size.x
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn find_formatter() -> Option<PathBuf> {
    let bundled = PathBuf::from(BUNDLED_FORMATTER);
    if bundled.is_file() {
        return Some(bundled);
    }
    which::which("sdFormatLinux").ok()
}

fn first_usable(devices: &[Device]) -> Option<usize> {
    devices.iter().position(|d| !d.system)
}

fn list_devices() -> Vec<Device> {
    let Ok(out) = Command::new("lsblk")
        .args(["-b", "-n", "-P", "-o", "NAME,PKNAME,TYPE,SIZE,MODEL,TRAN,MOUNTPOINT", "-e", "7"])
        .output()
    else {
        return vec![];
    };

    let text = String::from_utf8_lossy(&out.stdout);
    let rows: Vec<HashMap<String, String>> = text.lines().map(parse_pairs).collect();
    let get = |r: &HashMap<String, String>, k: &str| r.get(k).cloned().unwrap_or_default();

    // Walk each system mount up through partitions, LUKS and LVM to its disk.
    let parent: HashMap<String, String> = rows.iter().map(|r| (get(r, "NAME"), get(r, "PKNAME"))).collect();
    let mut system = HashSet::new();
    for r in rows.iter().filter(|r| SYSTEM_MOUNTS.contains(&get(r, "MOUNTPOINT").as_str())) {
        let mut name = get(r, "NAME");
        while let Some(p) = parent.get(&name).filter(|p| !p.is_empty()) {
            name = p.clone();
        }
        system.insert(name);
    }

    let mut devices: Vec<Device> = rows
        .iter()
        .filter(|r| get(r, "TYPE") == "disk")
        .filter_map(|r| {
            let name = get(r, "NAME");
            let size = get(r, "SIZE").parse().unwrap_or(0);
            // zram is swap in RAM; size 0 is a card reader with no card in it.
            if name.starts_with("zram") || size == 0 {
                return None;
            }
            Some(Device {
                path: format!("/dev/{name}"),
                model: get(r, "MODEL").trim().to_string(),
                tran: get(r, "TRAN"),
                size,
                system: system.contains(&name),
            })
        })
        .collect();
    // Removable media first; sort is stable so lsblk order holds otherwise.
    devices.sort_by_key(|d| d.system);
    devices
}

/// Parses one `lsblk -P` line: KEY="value" pairs, values using \xHH escapes.
fn parse_pairs(line: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut rest = line.trim();
    while let Some(eq) = rest.find("=\"") {
        let key = rest[..eq].trim().to_string();
        let after = &rest[eq + 2..];
        let Some(end) = after.find('"') else { break };
        map.insert(key, unescape_hex(&after[..end]));
        rest = &after[end + 1..];
    }
    map
}

fn unescape_hex(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'x') {
            if let Some(v) = s.get(i + 2..i + 4).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_output();
        if self.running {
            ui.ctx().request_repaint();
        }
        self.poll_update();

        // Actions anchored to the bottom
        egui::Panel::bottom("actions")
            .frame(
                egui::Frame::default()
                    .fill(BG)
                    .inner_margin(egui::Margin { left: 20, right: 20, top: 14, bottom: 10 }),
            )
            .show_inside(ui, |ui| self.actions_ui(ui));

        // Main content fills remaining space
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::same(20)))
            .show_inside(ui, |ui| {
                self.header_ui(ui);
                ui.add_space(12.0);
                self.outcome_ui(ui);
                ui.add_enabled_ui(!self.running, |ui| {
                    self.device_ui(ui);
                    ui.add_space(12.0);
                    self.label_ui(ui);
                });
                ui.add_space(12.0);
                self.output_ui(ui);
            });
    }
}

fn setup_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = INSET;
    v.faint_bg_color = SURFACE;
    v.hyperlink_color = LINK;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.window_corner_radius = egui::CornerRadius::same(8);
    v.selection.bg_fill = SELECT_BG;
    v.selection.stroke = Stroke::new(1.0_f32, LINK);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke.color = TEXT_LABEL;
    let w = &mut v.widgets;
    for (state, fill, stroke) in [
        (&mut w.inactive, CONTROL, BORDER),
        (&mut w.hovered, BORDER, MUTED),
        (&mut w.active, BORDER, LINK),
        (&mut w.open, BORDER, LINK),
    ] {
        state.weak_bg_fill = fill;
        state.bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0_f32, stroke);
        state.fg_stroke.color = TEXT;
        state.corner_radius = egui::CornerRadius::same(6);
    }
    ctx.set_visuals(v);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.interact_size.y = 34.0;
    ctx.set_global_style(style);
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("sdFormatLinux-Gui")
            .with_inner_size([560.0, 620.0])
            .with_min_inner_size([460.0, 520.0])
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "sdFormatLinux-Gui",
        options,
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(App::new(&cc.egui_ctx)))
        }),
    )
    .unwrap();
}
