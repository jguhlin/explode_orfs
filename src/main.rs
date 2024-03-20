use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssetUsages,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::VisibilitySystems,
    },
};
// use bevy_inspector_egui::quick::WorldInspectorPlugin;
use bevy_async_task::{AsyncTaskRunner, AsyncTaskStatus};
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use bevy_xpbd_3d::prelude::*;
use ffforf::*;
use fffx::Fasta;
use rand::prelude::*;
use rfd::AsyncFileDialog;

use std::collections::VecDeque;
use std::f32::consts::PI;
use std::time::Duration;

// Enum that will be used as a global state for the game
#[derive(Clone, Copy, Default, Eq, PartialEq, Debug, Hash, States)]
pub enum AppState {
    #[default]
    Menu,
    Run,
}

#[derive(Resource)]
pub struct Config {
    pub orfs_to_pop_per_step: usize,
    pub orf_material: Handle<StandardMaterial>,
    pub orf_length_max: usize,
    pub orf_length_min: usize,
    pub culling: usize,
    pub genome: Genome,
}

pub enum Genome {
    Nasonia,
    Custom(Vec<u8>),
}

impl PartialEq for Genome {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Genome::Nasonia, Genome::Nasonia) => true,
            (Genome::Custom(_), Genome::Custom(_)) => true,
            _ => false,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            orfs_to_pop_per_step: 28,
            orf_material: Handle::default(),
            orf_length_max: usize::MAX,
            orf_length_min: 100,
            culling: 2000,
            genome: Genome::Nasonia,
        }
    }
}

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        present_mode: bevy::window::PresentMode::AutoNoVsync, // Reduces input lag.
                        resizable: false,
                        resolution: (1280., 720.).into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_systems(Startup, startup)
        .add_plugins(EguiPlugin)
        // .add_plugins(WorldInspectorPlugin::new())
        .add_plugins(PhysicsPlugins::default())
        .insert_resource(Config::default())
        .insert_resource(Gravity(Vec3::new(0.0, 0.0, 0.0)))
        .insert_resource(SubstepCount(2))
        .add_systems(Update, gui.run_if(in_state(AppState::Menu)))
        .add_systems(OnEnter(AppState::Run), startup_data)
        .add_systems(
            Update,
            pop_orf_from_the_end_spiral_animation.run_if(in_state(AppState::Run)),
        )
        .add_systems(
            PostUpdate,
            cull.after(VisibilitySystems::CheckVisibility)
                .run_if(in_state(AppState::Run)),
        )
        .init_state::<AppState>()
        .run();
}

pub fn startup(mut commands: Commands,
) {
        // Camera!
        let e = commands.spawn(Camera3dBundle {
            transform: Transform::from_xyz(0.0, 6., 26.0).looking_at(Vec3::new(0., 1., 0.), Vec3::Y),
            ..default()
        }).id();
}

pub fn gui(
    mut contexts: EguiContexts,
    mut config: ResMut<Config>,
    mut commands: Commands,
    mut app_state: ResMut<NextState<AppState>>,
    mut task_executor: AsyncTaskRunner<Vec<u8>>,
) {
    egui::Window::new("Settings")        
        .default_width(400.0)
        .pivot(bevy_egui::egui::Align2::CENTER_CENTER)
        .show(contexts.ctx_mut(), |ui| {

        // Radio button
        ui.horizontal(|ui| {
            ui.radio_value(&mut config.genome, Genome::Nasonia, "Nasonia");
            ui.radio_value(&mut config.genome, Genome::Custom(Vec::new()), "Custom");
        });

        // If custom, have an upload button
        if let Genome::Custom(_) = config.genome {
            ui.label("Uploads may be plain fasta or gzip'd");
            if ui.button("Upload").clicked() {
                task_executor.start(async {
                    let task = AsyncFileDialog::new()
                        .add_filter("fasta", &["fasta", "fna", "fasta.gz", "fna.gz"])
                        .pick_file();

                        let file_handle = task.await.unwrap();
                        let file = file_handle.read().await;
                        return file

                });
            }
        }

        match task_executor.poll() {
            AsyncTaskStatus::Idle => (),
            AsyncTaskStatus::Pending => (),
            AsyncTaskStatus::Finished(v) => {
                println!("Got data");
                config.genome = Genome::Custom(v);
            }
        }

        // Min orf size
        ui.horizontal(|ui| {
            ui.label("Min orf size");
            ui.add(egui::Slider::new(&mut config.orf_length_min, 1..=1000));
        });

        // if orf_length_min < 50, issue a warning
        if config.orf_length_min < 50 {
            ui.label("Gonna be fun! orf length is less than 50 - It'll be slow");
        }

        // Culling
        #[cfg(target_arch = "wasm32")]
        {
            ui.horizontal(|ui| {
                ui.label("Culling");
                ui.add(egui::Slider::new(&mut config.culling, 100..=10_000));
            });
        }
        #[cfg(target_arch = "wasm32")]
        ui.label("Culling is how many elements can be on screen at once. Framerate drops when it gets too low.");

        // Row
        ui.horizontal(|ui| {
            ui.label("Orfs to pop per step");
            ui.add(egui::Slider::new(&mut config.orfs_to_pop_per_step, 1..=100));
        });
        ui.label("Increases the density of the ORF cloud");

        // Reset button
        if ui.button("Reset").clicked() {
            commands.insert_resource(Config::default());
        }

        if let Genome::Custom(ref data) = config.genome {
            if data.is_empty() {
                ui.label("No genome data! Won't start!");
            }
        }

        // Start button
        if ui.button("Start").clicked() {
            // If genome data is not nasonia and is empty, don't start
            if let Genome::Custom(ref data) = config.genome {
                if data.is_empty() {
                    return;
                }
            }
            app_state.set(AppState::Run);
        }
    });
}

// Despawn when off screen (not visible)
pub fn cull(
    mut commands: Commands,
    query: Query<(Entity, &ViewVisibility), With<OrfInSpace>>,
    mut orfs: ResMut<Orfs>,
    config: Res<Config>,
) {
    for (entity, view_visibility) in query.iter() {
        if !view_visibility.get() {
            // If it's not visible, despawn it
            commands.entity(entity).despawn_recursive();
        }
    }

    // If wasm, remove so we only have 1k orfs at a time
    #[cfg(target_arch = "wasm32")]
    {
        while orfs.entities.len() > config.culling {
            if let Some(entity) = orfs.entities.pop_front() {
                commands.entity(entity).despawn_recursive();
            }
        }
    }
}

#[derive(Resource)]
pub struct Orfs {
    orfs: VecDeque<Orf>,
    timer: Timer,
    chromosome_length: usize,
    random_list: Vec<usize>,
    entities: VecDeque<Entity>,
}

#[derive(Component)]
pub struct OrfInSpace;

#[derive(Resource)]
pub struct OrfNumber(usize);

pub fn pop_orf_from_the_end_spiral_animation(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut orfs: ResMut<Orfs>,
    time: Res<Time>,
    mut orf_number: ResMut<OrfNumber>,
    config: Res<Config>,
) {
    orfs.timer.tick(time.delta());

    if orfs.timer.finished() {
        // Pop the last orf
        // NOT doing random at this stage

        let mut front = false;

        for _ in 0..config.orfs_to_pop_per_step {
            let orf = if front {
                // front = !front;
                orfs.orfs.pop_front()
            } else {
                // front = !front;
                orfs.orfs.pop_back()
            };

            let orf = match orf {
                None => return,
                Some(orf) => orf,
            };

            let orf_length = orf.end - orf.start;
            let size = (orf_length as f32 / config.orf_length_max as f32) * 2.0 + 0.1;
            let color = Color::CYAN;

            let cylinder = meshes.add(Cylinder::new(0.15, size));
            let color = materials.add(StandardMaterial {
                base_color: color,
                ..default()
            });

            // Each orf shoots off in a spiral from the end of the chromosome, based
            // on which number it is in the list determines the proper angle
            let angle = orf_number.0 as f32 * 0.1;
            orf_number.0 += 1;

            let velocity = Vec3::new(0.0, angle.cos() * 6.0, angle.sin() * 6.0);

            // Place it at the start of the orf on the chromosome (chromosome is centered 0,0,0, length is in orfs.chromosome_length)
            // Because it is centered, those left of the center will be negative in x
            let x = (orf.start as f32 - orfs.chromosome_length as f32 / 2.0) / 1_000_000.0;

            let id = commands
                .spawn((
                    RigidBody::Dynamic,
                    // Collider::cylinder(0.1, size),
                    MassPropertiesBundle::new_computed(&Collider::cylinder(0.1, size), 2.5),
                    LinearVelocity(velocity),
                    PbrBundle {
                        mesh: cylinder,
                        material: color,
                        transform: Transform::from_xyz(x, 0.0, 0.0)
                            .with_rotation(Quat::from_rotation_z(-PI / 2.)),
                        ..default()
                    },
                    OrfInSpace,
                ))
                .id();

            orfs.entities.push_back(id);
        }
    }
}

pub fn startup_data(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut config: ResMut<Config>,
) {
    // Lights!
    commands.spawn(PointLightBundle {
        point_light: PointLight {
            shadows_enabled: true,
            intensity: 10_000_000.,
            range: 100.0,
            ..default()
        },
        transform: Transform::from_xyz(8.0, 16.0, 8.0),
        ..default()
    });

    // Actions are in another system though...

    // Set material
    let debug_material = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(uv_debug_texture())),
        ..default()
    });

    let file_contents = include_bytes!(
        "../data/Nasonia_vitripennis.Nvit_psr_1.1.dna.primary_assembly.CM020934.1.fa.gz"
    );

    let file_contents = file_contents.to_vec();

    let bytes = match config.genome {
        Genome::Nasonia => file_contents,
        Genome::Custom(ref bytes) => bytes.clone(),
    };

    // Test if gzip compressed
    let mut buf_reader: Box<std::io::BufReader<dyn std::io::Read>> = if bytes[0..2] == [0x1f, 0x8b]
    {
        // It's gzipped
        let decompressed = flate2::read::GzDecoder::new(&bytes[..]);
        Box::new(std::io::BufReader::new(decompressed))
    } else {
        Box::new(std::io::BufReader::new(&bytes[..]))
    };

    let mut reader = Fasta::from_buffer(&mut buf_reader);

    let record = reader.next().expect("record").expect("record");
    let seq = record.sequence.expect("sequence");

    let sequence_length = seq.len();

    // Let's add a cylinder to represent the chromosome, where every 100kbp is 1 unit
    let cylinder = meshes.add(Cylinder::new(0.1, sequence_length as f32 / 1_000_000.0));

    commands.spawn((
        RigidBody::Static,
        // Collider::cylinder(0.1, sequence_length as f32 / 1_000_000.0),
        PbrBundle {
            mesh: cylinder,
            material: debug_material.clone(),
            // Let's place is flat, so it lies along lengthwise like a stick where you see the wide side
            transform: Transform::from_xyz(0.0, 0.0, 0.0)
                .with_rotation(Quat::from_rotation_z(-PI / 2.)),
            ..default()
        },
        Chromosome,
    ));

    // Let's pull out all the ORFs for display later... save as a resource right now...

    let orf_min = config.orf_length_min;

    let mut all_orfs = find_all_orfs(&seq, orf_min);
    all_orfs.sort_by(|a, b| a.start.cmp(&b.start));
    let mut random_list = (0..all_orfs.len()).collect::<Vec<usize>>();
    let mut rng = rand::thread_rng();
    random_list.as_mut_slice().shuffle(&mut rng);

    // Calc orf lengths so we can get the min and max
    let mut orf_lengths = all_orfs
        .iter()
        .map(|orf| orf.end - orf.start)
        .collect::<Vec<usize>>();
    let orf_length_max = orf_lengths.iter().max().unwrap();
    let orf_length_min = orf_lengths.iter().min().unwrap();

    config.orf_length_max = *orf_length_max;
    config.orf_length_min = *orf_length_min;

    let orfs = Orfs {
        orfs: all_orfs.into(),
        timer: Timer::new(Duration::from_secs(2), TimerMode::Once),
        chromosome_length: sequence_length,
        random_list,
        entities: VecDeque::new(),
    };

    drop(reader);

    commands.insert_resource(orfs);
    commands.insert_resource(OrfNumber(0));

    let color = materials.add(StandardMaterial {
        base_color: Color::CYAN,
        ..default()
    });

    config.orf_material = color;
}

/// A marker component for our shapes so we can query them separately from the ground plane
#[derive(Component)]
struct Chromosome;

/// Creates a colorful test pattern
fn uv_debug_texture() -> Image {
    const TEXTURE_SIZE: usize = 8;

    let mut palette: [u8; 32] = [
        255, 102, 159, 255, 255, 159, 102, 255, 236, 255, 102, 255, 121, 255, 102, 255, 102, 255,
        198, 255, 102, 198, 255, 255, 121, 102, 255, 255, 236, 102, 255, 255,
    ];

    let mut texture_data = [0; TEXTURE_SIZE * TEXTURE_SIZE * 4];
    for y in 0..TEXTURE_SIZE {
        let offset = TEXTURE_SIZE * y * 4;
        texture_data[offset..(offset + TEXTURE_SIZE * 4)].copy_from_slice(&palette);
        palette.rotate_right(4);
    }

    Image::new_fill(
        Extent3d {
            width: TEXTURE_SIZE as u32,
            height: TEXTURE_SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &texture_data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
