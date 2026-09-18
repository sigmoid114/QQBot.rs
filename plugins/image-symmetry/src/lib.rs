use image::{
    AnimationDecoder, Delay, Frame, ImageFormat, RgbaImage,
    codecs::{
        gif::{GifDecoder, GifEncoder},
        png::PngDecoder,
        webp::WebPDecoder,
    },
    guess_format,
};
use kovi::{
    Message, PluginBuilder as P, RuntimeBot, serde_json,
    tokio::sync::RwLock,
    utils::{load_json_data, save_json_data},
};
use kovi_milky::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    error::Error,
    fs::{File, create_dir_all, remove_file},
    io::Cursor,
    path::PathBuf,
    sync::{Arc, LazyLock},
    time::Duration,
};
use uuid::Uuid;

static MANAGEMENT_CMD_REG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\$(.*)([+-])=image[-_]symmetry$").unwrap());
static SYMMETRY_CMD_REG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*对称\s+(\S+)\s*$").unwrap());
static BOT: LazyLock<Arc<RuntimeBot>> = LazyLock::new(|| P::get_runtime_bot());
static DATA_PATH: LazyLock<PathBuf> = LazyLock::new(|| BOT.get_data_path().join("data.json"));
static CACHE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let cache_dir = BOT.get_data_path().join("cache/");
    create_dir_all(&*cache_dir).unwrap();
    cache_dir
});
static DATA: LazyLock<RwLock<Data>> =
    LazyLock::new(|| match load_json_data(Data::new(), DATA_PATH.clone()) {
        Ok(data) => RwLock::new(data),
        Err(e) => {
            println!("插件 image-symmetry 数据格式错误(自动清空)：{}", e);
            save_json_data(&Data::new(), DATA_PATH.clone()).unwrap();
            RwLock::new(Data::new())
        }
    });

type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Serialize, Deserialize)]
struct Data {
    pub groups: HashSet<String>,
}

impl Data {
    fn new() -> Self {
        Self {
            groups: HashSet::new(),
        }
    }
}

enum Image {
    Static(RgbaImage),
    Dynamic(Vec<Frame>),
}

#[kovi::plugin]
async fn main() {
    let _ = &*BOT;
    P::on_admin_msg(move |event| on_admin_msg(event));
    P::on_group_msg(move |event| on_group_msg(event));
}

async fn on_admin_msg(event: Arc<AdminMsgEvent>) {
    if let Some(text) = event.borrow_text() {
        let cmd: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if let Some(caps) = MANAGEMENT_CMD_REG.captures(&cmd) {
            let mut data = DATA.write().await;
            if &caps[2] == "+" {
                data.groups.insert(String::from(&caps[1]));
            } else {
                data.groups.remove(&caps[1]);
            }
            save_json_data(&*data, &*DATA_PATH).unwrap();
        }
    }
}

async fn on_group_msg(event: Arc<GroupMsgEvent>) {
    let group_id = event.data.group.group_id;
    if {
        let data = DATA.read().await;
        data.groups.contains(&group_id.to_string())
    } && let Some(text) = event.borrow_text()
        && let Some(caps) = SYMMETRY_CMD_REG.captures(&text)
        && let Some(url) = get_image_url(event.data.segments.clone(), true)
    {
        match {
            match fetch_img(&url).await {
                Ok(Some(bytes)) => match dispatch(&bytes) {
                    Ok(Image::Static(image)) => case_static(&image, &caps[1]),
                    Ok(Image::Dynamic(frames)) => case_dynamic(&frames, &caps[1]),
                    Err(e) => Err(e),
                },
                Ok(None) => Err("未能识别到图片".into()),
                Err(e) => Err(e),
            }
        } {
            Ok((path, uri)) => {
                let _ = BOT
                    .send_group_message(
                        group_id,
                        Message::new()
                            .add_reply(event.data.message_seq)
                            .add_image(&uri),
                    )
                    .await;
                if let Err(e) = remove_file(path) {
                    println!("删除缓存失败：{}", e);
                }
            }
            Err(e) => {
                let s = format!("对称失败: {}", e);
                println!("{}", s);
                event.reply_and_quote(s);
            }
        }
    }
}

fn get_image_url(segments: MilkyMessage, recrusive: bool) -> Option<String> {
    for segment in segments {
        match segment.type_.as_str() {
            "image" => {
                if let Some(url) = segment.data.get("temp_url")
                    && let Some(url) = url.as_str()
                {
                    return Some(url.to_string());
                }
            }
            "reply" => {
                if recrusive
                    && let Some(segments) = segment.data.get("segments")
                    && let Ok(segments) = serde_json::from_value::<MilkyMessage>(segments.clone())
                    && let Some(url) = get_image_url(segments, false)
                {
                    return Some(url);
                }
            }
            _ => {}
        }
    }
    None
}

async fn fetch_img(url: &str) -> Result<Option<Vec<u8>>, BoxError> {
    let res = reqwest::get(url).await?;
    if !res.status().is_success() {
        return Ok(None);
    }
    let bytes = res.bytes().await?.to_vec();
    Ok(Some(bytes))
}

fn dispatch(bytes: &[u8]) -> Result<Image, BoxError> {
    match guess_format(&bytes) {
        Ok(ImageFormat::Gif) => {
            let decoder = GifDecoder::new(Cursor::new(&bytes))?;
            Ok(Image::Dynamic(decoder.into_frames().collect_frames()?))
        }
        Ok(ImageFormat::Png) => {
            let decoder = PngDecoder::new(Cursor::new(&bytes))?;
            if decoder.is_apng()? {
                Ok(Image::Dynamic(
                    decoder.apng()?.into_frames().collect_frames()?,
                ))
            } else {
                let img = image::load_from_memory(&bytes).unwrap();
                Ok(Image::Static(img.into_rgba8()))
            }
        }
        Ok(ImageFormat::WebP) => {
            let decoder = WebPDecoder::new(Cursor::new(&bytes))?;
            if decoder.has_animation() {
                Ok(Image::Dynamic(
                    decoder
                        .into_frames()
                        .take_while(|frame| frame.is_ok())
                        .filter_map(|frame| frame.ok())
                        .collect(),
                ))
            } else {
                let img = image::load_from_memory(&bytes).unwrap();
                Ok(Image::Static(img.into_rgba8()))
            }
        }
        _ => {
            let img = image::load_from_memory(&bytes).unwrap();
            Ok(Image::Static(img.into_rgba8()))
        }
    }
}

fn case_static(image: &RgbaImage, direction: &str) -> Result<(PathBuf, String), BoxError> {
    let image = symmetrize_static(image, direction);
    let path = CACHE_DIR.join(format!("{}.png", Uuid::new_v4()));
    let uri = format!("file://{}", path.to_string_lossy());
    image.save(&path)?;
    Ok((path, uri))
}

fn case_dynamic(frames: &[Frame], direction: &str) -> Result<(PathBuf, String), BoxError> {
    let frames = symmetrize_dynamic(frames, direction)?;
    let path = CACHE_DIR.join(format!("{}.gif", Uuid::new_v4()));
    let uri = format!("file://{}", path.to_string_lossy());
    let mut encoder = GifEncoder::new(File::create(&path)?);
    encoder.encode_frames(frames)?;
    Ok((path, uri))
}

fn symmetrize_static(origin: &RgbaImage, direction: &str) -> RgbaImage {
    let (width, height) = origin.dimensions();
    let mut container: Vec<u8> = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let px = match direction {
                "左" => origin.get_pixel(x.min(width - x - 1), y),
                "右" => origin.get_pixel(x.max(width - x - 1), y),
                "上" => origin.get_pixel(x, y.min(height - y - 1)),
                "下" => origin.get_pixel(x, y.max(height - y - 1)),
                _ => origin.get_pixel(x.min(width - x - 1), y),
            };
            let [r, g, b, a] = px.0;
            container.push(r);
            container.push(g);
            container.push(b);
            container.push(a);
        }
    }
    RgbaImage::from_raw(width, height, container).unwrap()
}

fn symmetrize_dynamic(frames: &[Frame], direction: &str) -> Result<Vec<Frame>, BoxError> {
    Ok(match direction {
        "左" | "右" | "上" | "下" => frames
            .iter()
            .map(|frame| {
                let buffer = symmetrize_static(frame.buffer(), direction);
                Frame::from_parts(buffer, frame.left(), frame.top(), frame.delay())
            })
            .collect(),
        "前" | "后" => {
            let time: Duration = frames
                .iter()
                .map(|frame| Duration::from(frame.delay()))
                .sum();
            let mut now = Duration::ZERO;
            let mut result: Vec<Frame> = Vec::new();
            for frame in {
                let iter: Box<dyn Iterator<Item = &Frame>> = if direction == "前" {
                    Box::new(frames.iter())
                } else {
                    Box::new(frames.iter().rev())
                };
                iter
            } {
                let next = now + frame.delay().into();
                if next < time / 2 {
                    result.push(frame.clone());
                    now = next;
                } else {
                    result.push(Frame::from_parts(
                        frame.buffer().clone(),
                        frame.left(),
                        frame.top(),
                        Delay::from_saturating_duration(time / 2 - now),
                    ));
                    break;
                }
            }
            result.extend(result.clone().into_iter().rev());
            result
        }
        _ => frames
            .iter()
            .map(|frame| {
                let buffer = symmetrize_static(frame.buffer(), "左");
                Frame::from_parts(buffer, frame.left(), frame.top(), frame.delay())
            })
            .collect(),
    })
}
