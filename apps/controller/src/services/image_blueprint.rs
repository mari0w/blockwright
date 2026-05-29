use crate::domain::types::{
    Blueprint, BlueprintBlock, BlueprintSize, ChatAttachment, ChatAttachmentKind,
    ChatAttachmentSource, MaterialCount,
};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const DEFAULT_LONG_EDGE: u32 = 192;
const HIGH_FIDELITY_LONG_EDGE: u32 = 256;
const MAX_LONG_EDGE: u32 = 512;
const MIN_LONG_EDGE: u32 = 16;
const TRANSPARENT_ALPHA_THRESHOLD: u8 = 16;

#[derive(Debug, Clone)]
pub struct ImageBlueprintPlan {
    pub blueprint: Blueprint,
    pub source_path: PathBuf,
    pub original_width: u32,
    pub original_height: u32,
    pub output_width: u32,
    pub output_height: u32,
}

#[derive(Debug)]
pub enum ImageBlueprintError {
    Decode {
        path: PathBuf,
        source: image::ImageError,
    },
    EmptyImage {
        path: PathBuf,
    },
    TooManyBlocks {
        requested: u64,
        max: u64,
    },
}

impl fmt::Display for ImageBlueprintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageBlueprintError::Decode { path, source } => {
                write!(formatter, "图片读取失败：{}，{source}", path.display())
            }
            ImageBlueprintError::EmptyImage { path } => {
                write!(formatter, "图片尺寸为空：{}", path.display())
            }
            ImageBlueprintError::TooManyBlocks { requested, max } => {
                write!(
                    formatter,
                    "图片复刻需要 {requested} 个方块，超过当前安全上限 {max}"
                )
            }
        }
    }
}

impl std::error::Error for ImageBlueprintError {}

#[derive(Clone, Copy)]
struct PaletteBlock {
    material: &'static str,
    rgb: [u8; 3],
}

const PALETTE: &[PaletteBlock] = &[
    PaletteBlock {
        material: "minecraft:white_concrete",
        rgb: [207, 213, 214],
    },
    PaletteBlock {
        material: "minecraft:light_gray_concrete",
        rgb: [125, 125, 115],
    },
    PaletteBlock {
        material: "minecraft:gray_concrete",
        rgb: [55, 58, 62],
    },
    PaletteBlock {
        material: "minecraft:black_concrete",
        rgb: [8, 10, 15],
    },
    PaletteBlock {
        material: "minecraft:brown_concrete",
        rgb: [96, 59, 31],
    },
    PaletteBlock {
        material: "minecraft:red_concrete",
        rgb: [142, 32, 32],
    },
    PaletteBlock {
        material: "minecraft:orange_concrete",
        rgb: [224, 97, 0],
    },
    PaletteBlock {
        material: "minecraft:yellow_concrete",
        rgb: [241, 175, 21],
    },
    PaletteBlock {
        material: "minecraft:lime_concrete",
        rgb: [94, 169, 24],
    },
    PaletteBlock {
        material: "minecraft:green_concrete",
        rgb: [73, 91, 36],
    },
    PaletteBlock {
        material: "minecraft:cyan_concrete",
        rgb: [21, 119, 136],
    },
    PaletteBlock {
        material: "minecraft:light_blue_concrete",
        rgb: [36, 137, 199],
    },
    PaletteBlock {
        material: "minecraft:blue_concrete",
        rgb: [44, 46, 143],
    },
    PaletteBlock {
        material: "minecraft:purple_concrete",
        rgb: [100, 32, 156],
    },
    PaletteBlock {
        material: "minecraft:magenta_concrete",
        rgb: [169, 48, 159],
    },
    PaletteBlock {
        material: "minecraft:pink_concrete",
        rgb: [214, 101, 143],
    },
    PaletteBlock {
        material: "minecraft:white_wool",
        rgb: [234, 236, 237],
    },
    PaletteBlock {
        material: "minecraft:light_gray_wool",
        rgb: [142, 142, 134],
    },
    PaletteBlock {
        material: "minecraft:gray_wool",
        rgb: [62, 68, 71],
    },
    PaletteBlock {
        material: "minecraft:black_wool",
        rgb: [20, 21, 25],
    },
    PaletteBlock {
        material: "minecraft:brown_wool",
        rgb: [114, 71, 40],
    },
    PaletteBlock {
        material: "minecraft:red_wool",
        rgb: [160, 39, 34],
    },
    PaletteBlock {
        material: "minecraft:orange_wool",
        rgb: [240, 118, 19],
    },
    PaletteBlock {
        material: "minecraft:yellow_wool",
        rgb: [248, 198, 39],
    },
    PaletteBlock {
        material: "minecraft:lime_wool",
        rgb: [112, 185, 25],
    },
    PaletteBlock {
        material: "minecraft:green_wool",
        rgb: [84, 109, 27],
    },
    PaletteBlock {
        material: "minecraft:cyan_wool",
        rgb: [21, 137, 145],
    },
    PaletteBlock {
        material: "minecraft:light_blue_wool",
        rgb: [58, 175, 217],
    },
    PaletteBlock {
        material: "minecraft:blue_wool",
        rgb: [53, 57, 157],
    },
    PaletteBlock {
        material: "minecraft:purple_wool",
        rgb: [121, 42, 172],
    },
    PaletteBlock {
        material: "minecraft:magenta_wool",
        rgb: [190, 68, 201],
    },
    PaletteBlock {
        material: "minecraft:pink_wool",
        rgb: [237, 141, 172],
    },
    PaletteBlock {
        material: "minecraft:white_terracotta",
        rgb: [210, 178, 161],
    },
    PaletteBlock {
        material: "minecraft:light_gray_terracotta",
        rgb: [135, 107, 98],
    },
    PaletteBlock {
        material: "minecraft:gray_terracotta",
        rgb: [58, 42, 36],
    },
    PaletteBlock {
        material: "minecraft:black_terracotta",
        rgb: [37, 22, 16],
    },
    PaletteBlock {
        material: "minecraft:brown_terracotta",
        rgb: [77, 51, 35],
    },
    PaletteBlock {
        material: "minecraft:red_terracotta",
        rgb: [143, 61, 47],
    },
    PaletteBlock {
        material: "minecraft:orange_terracotta",
        rgb: [161, 83, 38],
    },
    PaletteBlock {
        material: "minecraft:yellow_terracotta",
        rgb: [186, 133, 35],
    },
    PaletteBlock {
        material: "minecraft:lime_terracotta",
        rgb: [103, 117, 52],
    },
    PaletteBlock {
        material: "minecraft:green_terracotta",
        rgb: [76, 83, 42],
    },
    PaletteBlock {
        material: "minecraft:cyan_terracotta",
        rgb: [86, 91, 91],
    },
    PaletteBlock {
        material: "minecraft:light_blue_terracotta",
        rgb: [113, 108, 137],
    },
    PaletteBlock {
        material: "minecraft:blue_terracotta",
        rgb: [74, 59, 91],
    },
    PaletteBlock {
        material: "minecraft:purple_terracotta",
        rgb: [118, 70, 86],
    },
    PaletteBlock {
        material: "minecraft:magenta_terracotta",
        rgb: [149, 88, 108],
    },
    PaletteBlock {
        material: "minecraft:pink_terracotta",
        rgb: [161, 78, 78],
    },
    PaletteBlock {
        material: "minecraft:sandstone",
        rgb: [216, 204, 157],
    },
    PaletteBlock {
        material: "minecraft:smooth_sandstone",
        rgb: [219, 208, 163],
    },
    PaletteBlock {
        material: "minecraft:quartz_block",
        rgb: [235, 229, 222],
    },
    PaletteBlock {
        material: "minecraft:smooth_stone",
        rgb: [158, 158, 158],
    },
    PaletteBlock {
        material: "minecraft:stone",
        rgb: [125, 125, 125],
    },
    PaletteBlock {
        material: "minecraft:deepslate",
        rgb: [74, 74, 79],
    },
    PaletteBlock {
        material: "minecraft:oak_planks",
        rgb: [162, 130, 78],
    },
    PaletteBlock {
        material: "minecraft:spruce_planks",
        rgb: [114, 84, 48],
    },
    PaletteBlock {
        material: "minecraft:dark_oak_planks",
        rgb: [66, 43, 20],
    },
    PaletteBlock {
        material: "minecraft:birch_planks",
        rgb: [192, 175, 121],
    },
    PaletteBlock {
        material: "minecraft:jungle_planks",
        rgb: [160, 115, 81],
    },
    PaletteBlock {
        material: "minecraft:cherry_planks",
        rgb: [226, 178, 172],
    },
];

pub fn should_generate_image_blueprint(
    text: &str,
    attachments: &[ChatAttachment],
    image_ai_available: bool,
) -> bool {
    if first_local_image_path(attachments).is_none() {
        return false;
    }

    let text = text.trim();
    if text.is_empty() {
        return !image_ai_available;
    }

    if wants_pixel_surface(text) {
        return true;
    }

    if image_ai_available && wants_3d_recreation(text) {
        return false;
    }

    let explicit_build_words = [
        "复刻",
        "还原",
        "照着",
        "按照",
        "根据图",
        "图片",
        "照片",
        "像素",
        "壁画",
        "蓝图",
        "建",
        "造",
        "做",
        "放",
        "人物",
        "建筑",
        "百分百",
        "高保真",
        "完全",
    ];
    !image_ai_available
        && explicit_build_words
            .iter()
            .any(|keyword| text.contains(keyword))
}

fn wants_pixel_surface(text: &str) -> bool {
    [
        "像素", "壁画", "贴图", "平面", "浮雕", "照片", "人物", "人像", "头像", "真人", "脸",
    ]
    .iter()
    .any(|keyword| text.contains(keyword))
}

fn wants_3d_recreation(text: &str) -> bool {
    [
        "建筑", "房子", "屋", "楼", "城堡", "别墅", "房间", "树屋", "室内", "3d", "3D", "三维",
        "立体",
    ]
    .iter()
    .any(|keyword| text.contains(keyword))
}

pub fn build_from_first_local_image(
    text: &str,
    attachments: &[ChatAttachment],
) -> Option<Result<ImageBlueprintPlan, ImageBlueprintError>> {
    let path = first_local_image_path(attachments)?;
    Some(build_from_image_path(text, &path))
}

fn build_from_image_path(
    text: &str,
    path: &Path,
) -> Result<ImageBlueprintPlan, ImageBlueprintError> {
    let image = image::open(path).map_err(|source| ImageBlueprintError::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    let (original_width, original_height) = image.dimensions();
    if original_width == 0 || original_height == 0 {
        return Err(ImageBlueprintError::EmptyImage {
            path: path.to_path_buf(),
        });
    }

    let long_edge = requested_long_edge(text);
    let (output_width, output_height) =
        scaled_dimensions(original_width, original_height, long_edge);
    let requested_blocks = u64::from(output_width) * u64::from(output_height);
    let max_blocks = u64::from(MAX_LONG_EDGE) * u64::from(MAX_LONG_EDGE);
    if requested_blocks > max_blocks {
        return Err(ImageBlueprintError::TooManyBlocks {
            requested: requested_blocks,
            max: max_blocks,
        });
    }

    let resized = resize_to_rgba(image, output_width, output_height);
    let (blocks, materials) = quantize_to_vertical_mural(&resized, output_width, output_height);
    let id = generated_blueprint_id(path);
    let blueprint = Blueprint {
        id,
        name: "图片高保真像素复刻".to_string(),
        description: format!(
            "由图片 {} 生成的 {}x{} 垂直像素复刻蓝图。",
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("image"),
            output_width,
            output_height
        ),
        size: BlueprintSize {
            width: output_width,
            height: output_height,
            depth: 1,
        },
        spec: Some(json!({
            "format": "blockwright.blueprint_spec.v1",
            "kind": image_kind(text),
            "source": "image_to_pixel_blueprint",
            "intent": text.trim(),
            "notes": "controller 按图片像素直接生成方块；坐标为相对坐标，x 为图片横向，y 为图片纵向反转后的高度，z=0。",
            "image": {
                "path": path.to_string_lossy(),
                "original_width": original_width,
                "original_height": original_height,
                "output_width": output_width,
                "output_height": output_height,
                "orientation": "vertical_mural"
            },
            "palette_size": PALETTE.len(),
            "primitives": []
        })),
        materials,
        blocks,
        tags: vec![
            "image_recreation".to_string(),
            "pixel_blueprint".to_string(),
            image_kind(text).to_string(),
        ],
        expanded_hash: None,
    };

    Ok(ImageBlueprintPlan {
        blueprint,
        source_path: path.to_path_buf(),
        original_width,
        original_height,
        output_width,
        output_height,
    })
}

fn first_local_image_path(attachments: &[ChatAttachment]) -> Option<PathBuf> {
    let mut seen = HashSet::new();
    for attachment in attachments {
        if attachment.kind != ChatAttachmentKind::Image {
            continue;
        }
        let ChatAttachmentSource::LocalPath { path } = &attachment.source else {
            continue;
        };
        let path = PathBuf::from(path);
        if !path.is_file() || !seen.insert(path.clone()) {
            continue;
        }
        return Some(path);
    }
    None
}

fn requested_long_edge(text: &str) -> u32 {
    if let Some(value) = explicit_dimension(text) {
        return value.clamp(MIN_LONG_EDGE, MAX_LONG_EDGE);
    }
    if ["百分百", "完全", "高保真", "高清", "细节", "不要在乎方块"]
        .iter()
        .any(|keyword| text.contains(keyword))
    {
        return HIGH_FIDELITY_LONG_EDGE;
    }
    DEFAULT_LONG_EDGE
}

fn explicit_dimension(text: &str) -> Option<u32> {
    let mut best = None;
    let mut current = String::new();
    for ch in text.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() {
            current.push(ch);
            continue;
        }
        if !current.is_empty() {
            if let Ok(value) = current.parse::<u32>() {
                if (MIN_LONG_EDGE..=MAX_LONG_EDGE).contains(&value) {
                    best = Some(best.map_or(value, |existing: u32| existing.max(value)));
                }
            }
            current.clear();
        }
    }
    best
}

fn scaled_dimensions(width: u32, height: u32, long_edge: u32) -> (u32, u32) {
    if width >= height {
        let scaled_height = ((height as f64 / width as f64) * long_edge as f64).round() as u32;
        (long_edge, scaled_height.max(1))
    } else {
        let scaled_width = ((width as f64 / height as f64) * long_edge as f64).round() as u32;
        (scaled_width.max(1), long_edge)
    }
}

fn resize_to_rgba(image: DynamicImage, width: u32, height: u32) -> image::RgbaImage {
    image
        .resize_exact(width, height, FilterType::Lanczos3)
        .to_rgba8()
}

fn quantize_to_vertical_mural(
    image: &image::RgbaImage,
    width: u32,
    height: u32,
) -> (Vec<BlueprintBlock>, Vec<MaterialCount>) {
    let mut error_buffer = image
        .pixels()
        .map(|pixel| {
            [
                f32::from(pixel[0]),
                f32::from(pixel[1]),
                f32::from(pixel[2]),
                f32::from(pixel[3]),
            ]
        })
        .collect::<Vec<_>>();
    let mut blocks = Vec::with_capacity((width as usize).saturating_mul(height as usize));
    let mut counts = HashMap::<String, u32>::new();

    for y in 0..height {
        for x in 0..width {
            let index = pixel_index(width, x, y);
            let rgba = error_buffer[index];
            if rgba[3] < f32::from(TRANSPARENT_ALPHA_THRESHOLD) {
                continue;
            }

            let color = [
                rgba[0].round().clamp(0.0, 255.0) as u8,
                rgba[1].round().clamp(0.0, 255.0) as u8,
                rgba[2].round().clamp(0.0, 255.0) as u8,
            ];
            let palette = nearest_palette_block(color);
            let output_y = i32::try_from(height - 1 - y).unwrap_or_default();
            blocks.push(BlueprintBlock {
                x: i32::try_from(x).unwrap_or_default(),
                y: output_y,
                z: 0,
                material: palette.material.to_string(),
            });
            *counts.entry(palette.material.to_string()).or_default() += 1;

            let error = [
                rgba[0] - f32::from(palette.rgb[0]),
                rgba[1] - f32::from(palette.rgb[1]),
                rgba[2] - f32::from(palette.rgb[2]),
            ];
            diffuse_error(&mut error_buffer, width, height, x, y, error);
        }
    }

    let mut materials = counts
        .into_iter()
        .map(|(material, count)| MaterialCount { material, count })
        .collect::<Vec<_>>();
    materials.sort_by(|left, right| left.material.cmp(&right.material));
    (blocks, materials)
}

fn pixel_index(width: u32, x: u32, y: u32) -> usize {
    (y as usize)
        .saturating_mul(width as usize)
        .saturating_add(x as usize)
}

fn diffuse_error(
    buffer: &mut [[f32; 4]],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    error: [f32; 3],
) {
    add_error(buffer, width, height, x + 1, y, error, 7.0 / 16.0);
    if x > 0 {
        add_error(buffer, width, height, x - 1, y + 1, error, 3.0 / 16.0);
    }
    add_error(buffer, width, height, x, y + 1, error, 5.0 / 16.0);
    add_error(buffer, width, height, x + 1, y + 1, error, 1.0 / 16.0);
}

fn add_error(
    buffer: &mut [[f32; 4]],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    error: [f32; 3],
    factor: f32,
) {
    if x >= width || y >= height {
        return;
    }
    let index = pixel_index(width, x, y);
    for (channel, value) in error.iter().enumerate() {
        buffer[index][channel] = (buffer[index][channel] + value * factor).clamp(0.0, 255.0);
    }
}

fn nearest_palette_block(color: [u8; 3]) -> PaletteBlock {
    PALETTE
        .iter()
        .copied()
        .min_by_key(|palette| color_distance(color, palette.rgb))
        .unwrap_or(PALETTE[0])
}

fn color_distance(left: [u8; 3], right: [u8; 3]) -> u32 {
    let dr = i32::from(left[0]) - i32::from(right[0]);
    let dg = i32::from(left[1]) - i32::from(right[1]);
    let db = i32::from(left[2]) - i32::from(right[2]);
    // 人眼对绿色更敏感，权重稍高；这对皮肤、树叶和建筑阴影更稳。
    (30 * dr * dr + 59 * dg * dg + 11 * db * db) as u32
}

fn generated_blueprint_id(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("image");
    let safe_stem = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(32)
        .collect::<String>();
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    if safe_stem.is_empty() {
        format!("image-pixel-{millis}")
    } else {
        format!("image-pixel-{safe_stem}-{millis}")
    }
}

fn image_kind(text: &str) -> &'static str {
    if ["人物", "人像", "头像", "照片", "真人"]
        .iter()
        .any(|keyword| text.contains(keyword))
    {
        "portrait_pixel_recreation"
    } else if ["建筑", "房子", "屋", "楼", "城堡"]
        .iter()
        .any(|keyword| text.contains(keyword))
    {
        "building_image_recreation"
    } else {
        "image_pixel_recreation"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_image_path(name: &str) -> PathBuf {
        let number = NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "blockwright-image-blueprint-{name}-{}-{number}.png",
            std::process::id()
        ))
    }

    #[test]
    fn generates_vertical_mural_with_relative_coordinates() {
        let path = temp_image_path("mural");
        let mut image = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(2, 2);
        image.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
        image.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
        image.put_pixel(0, 1, Rgba([255, 0, 0, 255]));
        image.put_pixel(1, 1, Rgba([0, 0, 255, 255]));
        image.save(&path).unwrap();

        let plan = build_from_image_path("按 16 像素复刻", &path).unwrap();

        assert_eq!(plan.output_width, 16);
        assert_eq!(plan.output_height, 16);
        assert_eq!(plan.blueprint.size.depth, 1);
        assert_eq!(plan.blueprint.blocks.len(), 256);
        assert!(plan
            .blueprint
            .blocks
            .iter()
            .all(|block| block.z == 0 && block.x >= 0 && block.y >= 0));
        assert!(plan.blueprint.tags.contains(&"pixel_blueprint".to_string()));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn high_fidelity_words_raise_default_size() {
        assert_eq!(
            requested_long_edge("百分百完全还原"),
            HIGH_FIDELITY_LONG_EDGE
        );
        assert_eq!(requested_long_edge("普通复刻"), DEFAULT_LONG_EDGE);
        assert_eq!(requested_long_edge("做 512x512"), MAX_LONG_EDGE);
    }

    #[test]
    fn detects_local_image_recreation_request() {
        let path = temp_image_path("detect");
        ImageBuffer::<Rgba<u8>, Vec<u8>>::new(1, 1)
            .save(&path)
            .unwrap();
        let attachments = vec![ChatAttachment {
            kind: ChatAttachmentKind::Image,
            source: ChatAttachmentSource::LocalPath {
                path: path.to_string_lossy().to_string(),
            },
            file_name: None,
            mime_type: None,
        }];

        assert!(should_generate_image_blueprint(
            "帮我复刻这张建筑图",
            &attachments,
            false
        ));
        assert!(!should_generate_image_blueprint(
            "帮我复刻这张建筑图",
            &attachments,
            true
        ));
        assert!(should_generate_image_blueprint(
            "帮我复刻这张人物照片",
            &attachments,
            true
        ));
        assert!(!should_generate_image_blueprint(
            "这是什么",
            &attachments,
            false
        ));

        let _ = std::fs::remove_file(path);
    }
}
