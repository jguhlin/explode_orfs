use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssetUsages,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::VisibilitySystems,
    },
};
use bevy_inspector_egui::quick::WorldInspectorPlugin;
use bevy_xpbd_3d::prelude::*;
use ffforf::*;
use needletail::parse_fastx_file;
use rand::prelude::*;

use std::f32::consts::PI;
use std::time::Duration;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        // .add_plugins(WorldInspectorPlugin::new())
        .add_plugins(PhysicsPlugins::default())
        .insert_resource(Gravity(Vec3::new(0.0, 0.0, 0.0)))
        .insert_resource(SubstepCount(2))
        .add_systems(Startup, startup_data)
        // .add_systems(Startup, setup)
        // .add_systems(Update, roll)
        .add_systems(Update, pop_random_orf)
                .insert_resource(SubstepCount(30))
        // .add_systems(PostUpdate, despawn_when_off_screen.after(VisibilitySystems::CheckVisibility))
        .run();
}

// Despawn when off screen (not visible)
pub fn despawn_when_off_screen(
    mut commands: Commands,
    query: Query<(Entity, &ViewVisibility), With<OrfInSpace>>,
) {
    for (entity, view_visibility) in query.iter() {
        if !view_visibility.get() {
            // If it's not visible, despawn it
            commands.entity(entity).despawn_recursive();
            println!("Despawning entity: {:?}", entity);
        }
    }
}

#[derive(Resource)]
pub struct Orfs {
    orfs: Vec<Orf>,
    timer: Timer,
    chromosome_length: usize,
    random_list: Vec<usize>,
}

#[derive(Component)]
pub struct OrfInSpace;

pub fn pop_random_orf(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut orfs: ResMut<Orfs>,
    time: Res<Time>,
) {
    orfs.timer.tick(time.delta());

    if orfs.timer.finished() {
        // Pop the last orf
        // NOT doing random at this stage
        if orfs.random_list.is_empty() {
            return;
        }

        let i = orfs.random_list.pop().unwrap();
        let orf = &orfs.orfs[i];       

        let orf_length = orf.end - orf.start;
        let size = 0.1_f32.max(orf_length as f32 / 1_000_000.0);

        let cylinder = meshes.add(Cylinder::new(0.1, size));
        let color = materials.add(StandardMaterial {
            base_color: Color::CYAN,
            ..default()
        });

        let mut rng = rand::thread_rng();

        // Random vec3 for velocity
        let mut velocity = Vec3::new(
            rng.gen_range(-2.0..2.0),
            rng.gen_range(-2.0..2.0),
            rng.gen_range(-2.0..2.0),
        );      

        // Place it at the start of the orf on the chromosome (chromosome is centered 0,0,0, length is in orfs.chromosome_length)
        // Because it is centered, those left of the center will be negative in x
        let x = (orf.start as f32 - orfs.chromosome_length as f32 / 2.0) / 1_000_000.0;

        
        commands.spawn((
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
        ));
    }
}

pub fn startup_data(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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

    // Camera!
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(0.0, 6., 26.0).looking_at(Vec3::new(0., 1., 0.), Vec3::Y),
        ..default()
    });

    // Actions are in another system though...

    // Set material
    let debug_material = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(uv_debug_texture())),
        ..default()
    });

    let file = "data/Nasonia_vitripennis.Nvit_psr_1.1.dna.primary_assembly.CM020934.1.fa.gz";
    let mut reader = parse_fastx_file(&file).expect("valid path/file");
    let record = reader.next().expect("record").expect("record");
    let seq = record.seq();

    let sequence_length = record.num_bases();

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


    let all_orfs = find_all_orfs(&seq, 50);
    let mut random_list = (0..all_orfs.len()).collect::<Vec<usize>>();
    let mut rng = rand::thread_rng();
    random_list.as_mut_slice().shuffle(&mut rng);

    let orfs = Orfs {
        orfs: all_orfs,
        timer: Timer::new(Duration::from_secs(5), TimerMode::Once),
        chromosome_length: sequence_length,
        random_list,
    };
    commands.insert_resource(orfs);
}

/// A marker component for our shapes so we can query them separately from the ground plane
#[derive(Component)]
struct Chromosome;

fn roll(mut query: Query<&mut Transform, With<Chromosome>>, time: Res<Time>) {
    // Don't rotate, but roll it along the x-axis
    for mut transform in &mut query {
        transform.rotate_x(time.delta_seconds() * 0.8)
    }
}

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
