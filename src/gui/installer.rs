use eframe::egui;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use crate::cli;
use crate::desktop::install_self;
use crate::manifest::parse_manifest;
use crate::storage::paths;

pub enum InstallType {
    SelfInstall,
    AppInstall {
        name: String,
        display_name: String,
        manifest_content: String,
    },
}

pub struct InstallerApp {
    install_type: InstallType,
    state: InstallerState,
    recv: Receiver<InstallStatus>,
    sender: Sender<InstallStatus>, // Kept to clone for the thread
}

enum InstallerState {
    Confirmation,
    Installing { progress: f32, message: String },
    Done { message: String },
    Error { message: String },
}

enum InstallStatus {
    Progress(f32, String),
    Success(String),
    Error(String),
}

impl InstallerApp {
    pub fn new(install_type: InstallType) -> Self {
        let (sender, recv) = channel();
        Self {
            install_type,
            state: InstallerState::Confirmation,
            recv,
            sender,
        }
    }

    fn start_installation(&mut self) {
        let sender = self.sender.clone();
        let install_type = match &self.install_type {
            InstallType::SelfInstall => InstallType::SelfInstall,
            InstallType::AppInstall {
                name,
                display_name,
                manifest_content,
            } => InstallType::AppInstall {
                name: name.clone(),
                display_name: display_name.clone(),
                manifest_content: manifest_content.clone(),
            },
        };

        self.state = InstallerState::Installing {
            progress: 0.0,
            message: "Starting installation...".to_string(),
        };

        thread::spawn(
            move || match perform_installation(install_type, sender.clone()) {
                Ok(msg) => {
                    let _ = sender.send(InstallStatus::Success(msg));
                }
                Err(e) => {
                    let _ = sender.send(InstallStatus::Error(e.to_string()));
                }
            },
        );
    }
}

fn perform_installation(
    install_type: InstallType,
    sender: Sender<InstallStatus>,
) -> Result<String, Box<dyn std::error::Error>> {
    match install_type {
        InstallType::SelfInstall => {
            let _ = sender.send(InstallStatus::Progress(
                0.1,
                "Creating directories...".to_string(),
            ));
            paths::ensure_dirs()?;

            let _ = sender.send(InstallStatus::Progress(
                0.5,
                "Copying binary...".to_string(),
            ));
            install_self()?;

            let _ = sender.send(InstallStatus::Progress(1.0, "Done!".to_string()));
            Ok(format!(
                "Voidbox v{} has been installed successfully!\n\nYou can now use 'voidbox' from your terminal.",
                crate::VERSION
            ))
        }
        InstallType::AppInstall {
            name,
            display_name,
            manifest_content,
        } => {
            let _ = sender.send(InstallStatus::Progress(
                0.1,
                format!("Preparing to install {}...", display_name),
            ));

            // Ensure runtime is installed first
            if !paths::install_path().exists() {
                let _ = sender.send(InstallStatus::Progress(
                    0.2,
                    "Installing Voidbox runtime...".to_string(),
                ));
                paths::ensure_dirs()?;
                install_self()?;
            }

            let _ = sender.send(InstallStatus::Progress(
                0.3,
                "Parsing manifest...".to_string(),
            ));
            let manifest = parse_manifest(&manifest_content)?;
            let manifest_path = paths::manifest_path(&name);

            // Save manifest
            paths::ensure_dirs()?;
            std::fs::write(&manifest_path, manifest_content)?;

            // We can't easily get granular progress from the CLI functions yet without refactoring,
            // so we'll just show indeterminate progress or "Installing..."
            let _ = sender.send(InstallStatus::Progress(
                0.5,
                "Downloading and extracting...".to_string(),
            ));

            // Install the app
            // Note: This blocks until done
            cli::install_app_from_manifest(&manifest, false)?;

            let _ = sender.send(InstallStatus::Progress(1.0, "Done!".to_string()));
            Ok(format!("{} has been installed successfully!", display_name))
        }
    }
}

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll for updates from the thread
        while let Ok(status) = self.recv.try_recv() {
            match status {
                InstallStatus::Progress(p, msg) => {
                    self.state = InstallerState::Installing {
                        progress: p,
                        message: msg,
                    };
                }
                InstallStatus::Success(msg) => {
                    self.state = InstallerState::Done { message: msg };
                }
                InstallStatus::Error(msg) => {
                    self.state = InstallerState::Error { message: msg };
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.heading("Voidbox Installer");
                ui.add_space(20.0);

                match &self.state {
                    InstallerState::Confirmation => {
                        match &self.install_type {
                            InstallType::SelfInstall => {
                                ui.label(format!("Install Voidbox v{}?", crate::VERSION));
                                ui.label("This will install voidbox to ~/.local/bin/voidbox");
                            }
                            InstallType::AppInstall { display_name, .. } => {
                                ui.label(format!("Install {}?", display_name));
                                ui.label(
                                    "This will download and install the application container.",
                                );
                            }
                        }
                        ui.add_space(30.0);

                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Install").clicked() {
                                        self.start_installation();
                                    }
                                    if ui.button("Cancel").clicked() {
                                        std::process::exit(0);
                                    }
                                },
                            );
                        });
                    }
                    InstallerState::Installing { progress, message } => {
                        ui.label(message);
                        ui.add_space(10.0);
                        ui.add(egui::ProgressBar::new(*progress).animate(true));
                    }
                    InstallerState::Done { message } => {
                        ui.label(message);
                        ui.add_space(20.0);
                        if ui.button("Close").clicked() {
                            std::process::exit(0);
                        }
                    }
                    InstallerState::Error { message } => {
                        ui.colored_label(egui::Color32::RED, "Installation Failed");
                        ui.label(message);
                        ui.add_space(20.0);
                        if ui.button("Close").clicked() {
                            std::process::exit(1);
                        }
                    }
                }
            });
        });
    }
}

pub fn run_installer(install_type: InstallType) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "Voidbox Installer",
        options,
        Box::new(|_cc| Ok(Box::new(InstallerApp::new(install_type)))),
    )
}
