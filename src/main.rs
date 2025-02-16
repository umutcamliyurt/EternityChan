#[macro_use]
extern crate rocket;

use rocket::serde::{json::Json, Deserialize, Serialize};
use rocket::fs::{FileServer, NamedFile, TempFile};
use rocket::State;
use uuid::Uuid;
use std::sync::Mutex;
use std::path::Path;
use std::fs::OpenOptions;
use std::io::{Write, Read};
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use argon2::Argon2;
use rand::RngCore;
use zeroize::Zeroize;
use rpassword::read_password;


#[derive(Serialize, Deserialize, Clone)]
struct Post {
    id: usize,
    user: String,
    content: String,
    media: Option<String>,
    iv: String,
    media_iv: Option<String>,
}

#[derive(Deserialize)]
struct NewPost {
    user: String,
    content: String,
    media: Option<String>,
    iv: String,
    media_iv: Option<String>,
}

type PostList = Mutex<Vec<Post>>;

struct AppState {
    password: String,
}

impl AppState {
    fn new(password: &str) -> Self {
        AppState {
            password: password.to_string(),
        }
    }
}

pub fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let argon2 = Argon2::default();
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .expect("Key derivation failed");
    key
}

pub fn encrypt_data(plain_text: &str, password: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);

    let mut key = derive_key(password, &salt);
    let cipher = ChaCha20Poly1305::new(&Key::from_slice(&key));

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let encrypted_data = cipher
        .encrypt(nonce, plain_text.as_bytes())
        .map_err(|_| "Encryption error")?;

    key.zeroize();

    Ok(format!(
        "{}:{}:{}",
        hex::encode(salt),
        hex::encode(nonce_bytes),
        hex::encode(encrypted_data)
    ))
}

pub fn decrypt_data(encrypted_text: &str, password: &str) -> Result<String, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = encrypted_text.split(':').collect();
    if parts.len() != 3 {
        return Err("Invalid encrypted data format".into());
    }

    let salt = hex::decode(parts[0]).map_err(|_| "Decryption error: Invalid salt format")?;
    let nonce_bytes = hex::decode(parts[1]).map_err(|_| "Decryption error: Invalid nonce format")?;
    let encrypted_data = hex::decode(parts[2]).map_err(|_| "Decryption error: Invalid encrypted data format")?;

    let mut key = derive_key(password, &salt);
    let cipher = ChaCha20Poly1305::new(&Key::from_slice(&key));

    let nonce = Nonce::from_slice(&nonce_bytes);

    let decrypted_data = cipher
        .decrypt(nonce, encrypted_data.as_ref())
        .map_err(|_| "Decryption error: Failed to decrypt")?;

    key.zeroize();

    Ok(String::from_utf8(decrypted_data).map_err(|_| "Decryption error: Invalid UTF-8 data")?)
}

fn save_posts_to_disk(posts: &Vec<Post>, password: &str) -> std::io::Result<()> {
    let encrypted_posts = posts
        .iter()
        .map(|post| {
            let encrypted_user = encrypt_data(&post.user, password).expect("Failed to encrypt username");
            let encrypted_content = encrypt_data(&post.content, password).expect("Failed to encrypt post content");
            let encrypted_media = match &post.media {
                Some(media) => Some(encrypt_data(media, password).expect("Failed to encrypt media")),
                None => None,
            };
            Post {
                id: post.id,
                user: encrypted_user,
                content: encrypted_content,
                media: encrypted_media,
                iv: post.iv.clone(),
                media_iv: post.media_iv.clone(),
            }
        })
        .collect::<Vec<_>>();

    let file_path = "posts.dat";
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(file_path)?;

    let data = serde_json::to_string(&encrypted_posts).expect("Failed to serialize posts");
    file.write_all(data.as_bytes())?;
    Ok(())
}

fn load_posts_from_disk(password: &str) -> std::io::Result<Vec<Post>> {
    let file_path = "posts.dat";
    if !Path::new(file_path).exists() {
        return Ok(Vec::new());
    }

    let mut file = OpenOptions::new().read(true).open(file_path)?;
    let mut data = String::new();
    file.read_to_string(&mut data)?;

    let encrypted_posts: Vec<Post> = serde_json::from_str(&data).expect("Failed to deserialize posts");
    
    let decrypted_posts = encrypted_posts
        .into_iter()
        .map(|post| {
            let decrypted_user = decrypt_data(&post.user, password).expect("Failed to decrypt username");
            let decrypted_content = decrypt_data(&post.content, password).expect("Failed to decrypt post content");
            let decrypted_media = match post.media {
                Some(media) => Some(decrypt_data(&media, password).expect("Failed to decrypt media")),
                None => None,
            };
            Post {
                id: post.id,
                user: decrypted_user,
                content: decrypted_content,
                media: decrypted_media,
                iv: post.iv,
                media_iv: post.media_iv,
            }
        })
        .collect();

    Ok(decrypted_posts)
}

fn get_user_password() -> String {
    println!("Please enter your password: ");
    read_password().unwrap()
}

#[get("/posts")]
fn get_posts(posts: &State<PostList>) -> Json<Vec<Post>> {
    let posts = posts.lock().unwrap();
    Json(posts.clone())
}

#[post("/posts", data = "<new_post>")]
fn create_post(
    new_post: Json<NewPost>,
    posts: &State<PostList>,
    app_state: &State<AppState>
) -> Result<Json<Post>, Json<String>> {
    if new_post.user.len() > 20 {
        return Err(Json("Username must be 20 characters or less".to_string()));
    }

    let mut posts_guard = posts.lock().unwrap();
    let post = Post {
        id: posts_guard.len() + 1,
        user: new_post.user.clone(),
        content: new_post.content.clone(),
        media: new_post.media.clone(),
        iv: new_post.iv.clone(),
        media_iv: new_post.media_iv.clone(),
    };
    posts_guard.push(post.clone());

    // Save the updated posts to disk.
    if let Err(e) = save_posts_to_disk(&*posts_guard, &app_state.password) {
        eprintln!("Failed to save posts to disk: {}", e);
    }
    
    Ok(Json(post))
}

#[post("/upload", data = "<file>")]
async fn upload(mut file: TempFile<'_>) -> Json<String> {
    let upload_path = Path::new("static/uploads");
    std::fs::create_dir_all(upload_path).unwrap();

    let filename = format!("{}.png", Uuid::new_v4());
    let file_path = upload_path.join(&filename);
    match file.move_copy_to(&file_path).await {
        Ok(_) => Json(format!("/static/uploads/{}", filename)),
        Err(e) => Json(format!("Error uploading file: {}", e)),
    }
}

#[get("/")]
async fn index() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/index.html")).await.ok()
}

#[launch]
fn rocket() -> _ {
    let user_password = get_user_password();

    let app_state = AppState::new(&user_password);

    let posts = load_posts_from_disk(&user_password).unwrap_or_else(|e| {
        eprintln!("Error loading posts: {}", e);
        Vec::new()
    });

    rocket::build()
        .manage(Mutex::new(posts))
        .manage(app_state)
        .mount("/api", routes![get_posts, create_post, upload])
        .mount("/", routes![index])
        .mount("/static", FileServer::from("static"))
}
