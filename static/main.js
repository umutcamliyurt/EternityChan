function getKeyFromUrl() {
    const keyHex = window.location.hash.substring(1); 
    if (keyHex.length === 0) {
        console.error("Encryption key not found in URL fragment!");
        return null;
    }
    const keyBytes = new Uint8Array(keyHex.match(/.{2}/g).map(byte => parseInt(byte, 16)));
    return crypto.subtle.importKey("raw", keyBytes, { name: "AES-GCM" }, false, ["encrypt", "decrypt"]);
}

async function generateAndStoreKey() {
    const key = await crypto.subtle.generateKey(
        {
            name: "AES-GCM",
            length: 256
        },
        true,
        ["encrypt", "decrypt"]
    );

    const exportedKey = await crypto.subtle.exportKey("raw", key);
    const keyHex = Array.from(new Uint8Array(exportedKey))
        .map(byte => byte.toString(16).padStart(2, "0"))
        .join("");

    window.location.hash = keyHex; 
    return key;
}

async function encryptContent(content, key) {
    const encoder = new TextEncoder();
    const encodedContent = encoder.encode(content);
    const iv = crypto.getRandomValues(new Uint8Array(12)); 
    const encryptedContent = await crypto.subtle.encrypt(
        { name: "AES-GCM", iv: iv },
        key,
        encodedContent
    );
    return {
        encryptedContent: bufferToHex(encryptedContent),
        iv: bufferToHex(iv)
    };
}

async function decryptContent(encryptedContentHex, ivHex, key) {
    const encryptedContent = hexToBuffer(encryptedContentHex);
    const iv = hexToBuffer(ivHex);
    const decrypted = await crypto.subtle.decrypt(
        { name: "AES-GCM", iv: iv },
        key,
        encryptedContent
    );
    const decoder = new TextDecoder();
    return decoder.decode(decrypted);
}

function bufferToHex(buffer) {
    const byteArray = new Uint8Array(buffer);
    return Array.from(byteArray)
        .map(byte => byte.toString(16).padStart(2, '0'))
        .join('');
}

function hexToBuffer(hex) {
    return new Uint8Array(hex.match(/.{2}/g).map(byte => parseInt(byte, 16)));
}

function arrayBufferToBase64(buffer) {
    let binary = '';
    const bytes = new Uint8Array(buffer);
    for (let i = 0; i < bytes.byteLength; i++) {
        binary += String.fromCharCode(bytes[i]);
    }
    return btoa(binary);
}

function base64ToArrayBuffer(base64) {
    const binaryString = atob(base64);
    const len = binaryString.length;
    const bytes = new Uint8Array(len);
    for (let i = 0; i < len; i++) {
        bytes[i] = binaryString.charCodeAt(i);
    }
    return bytes.buffer;
}

async function encryptMedia(mediaFile, key) {
    const arrayBuffer = await mediaFile.arrayBuffer();
    const base64String = arrayBufferToBase64(arrayBuffer);
    const encoder = new TextEncoder();
    const encodedBase64 = encoder.encode(base64String);

    const iv = crypto.getRandomValues(new Uint8Array(12)); 
    const encryptedMediaBuffer = await crypto.subtle.encrypt(
        { name: "AES-GCM", iv: iv },
        key,
        encodedBase64
    );
    return {
        encryptedMedia: bufferToHex(encryptedMediaBuffer),
        mediaIv: bufferToHex(iv)
    };
}

async function decryptMedia(encryptedMediaHex, ivHex, key) {
    const encryptedMedia = hexToBuffer(encryptedMediaHex);
    const iv = hexToBuffer(ivHex);
    const decryptedBuffer = await crypto.subtle.decrypt(
        { name: "AES-GCM", iv: iv },
        key,
        encryptedMedia
    );
    const decoder = new TextDecoder();
    const base64String = decoder.decode(decryptedBuffer);
    const originalBuffer = base64ToArrayBuffer(base64String);
    return new Blob([originalBuffer]);
}

function updateMediaLabel() {
    const fileInput = document.getElementById('media');
    const fileName = fileInput.files.length > 0 ? fileInput.files[0].name : 'Select Media';
    document.getElementById('selectMediaBtn').textContent = fileName;
}

async function fetchPosts(key) {
    try {
        const response = await fetch('/api/posts');
        const posts = await response.json();
        const postElements = await Promise.all(posts.map(async post => {
            try {
                const decryptedContent = await decryptContent(post.content, post.iv, key);
                let decryptedMedia = null;
                if (post.media) {
                    decryptedMedia = await decryptMedia(post.media, post.media_iv, key);
                }
                return createPostElement(post, decryptedContent, decryptedMedia);
            } catch (err) {
                console.error("Decryption error for post:", post, err);
                return null; 
            }
        }));

        const validPostElements = postElements.filter(post => post !== null);
        document.getElementById('posts').innerHTML = validPostElements.join('');
    } catch (err) {
        console.error("Error fetching posts:", err);
    }
}

const dompurifyConfig = {
    ALLOWED_TAGS: ['b', 'i', 'em', 'strong', 'p', 'br', 'ul', 'ol', 'li', 'a', 'img'],
    ALLOWED_ATTR: ['href', 'title', 'target', 'src', 'alt', 'width', 'height']
};

function createPostElement(post, decryptedContent, decryptedMedia) {
    // Trim the username to 20 characters
    const username = post.user && post.user.length > 20 ? post.user.substring(0, 20) : post.user || "Anonymous";

    let postHtml = `
        <div class="post">
            <strong>${username}</strong>
            <p>${DOMPurify.sanitize(decryptedContent, dompurifyConfig)}</p>
    `;
    if (decryptedMedia) {
        const objectUrl = URL.createObjectURL(decryptedMedia);
        postHtml += `<img src="${DOMPurify.sanitize(objectUrl, dompurifyConfig)}" onclick="openModal('${objectUrl}')">`;
    }
    postHtml += `</div>`;
    return postHtml;
}

async function createPost() {
    const user = document.getElementById('user').value || "Anonymous";
    const content = document.getElementById('content').value;
    const mediaFile = document.getElementById('media').files[0];  

    if (!content && !mediaFile) return;  

    const key = await getKeyFromUrl();

    if (key) {
        const { encryptedContent, iv } = await encryptContent(content, key);

        let encryptedMedia = null;
        let mediaIv = null;
        if (mediaFile) {
            const result = await encryptMedia(mediaFile, key);
            encryptedMedia = result.encryptedMedia;
            mediaIv = result.mediaIv;
        }

        const post = { 
            user, 
            content: encryptedContent, 
            iv, 
            media: encryptedMedia, 
            media_iv: mediaIv 
        };

        await fetch('/api/posts', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(post)
        });

        document.getElementById('content').value = '';
        document.getElementById('user').value = '';
        document.getElementById('media').value = '';
        document.getElementById('selectMediaBtn').textContent = 'Select Media';

        fetchPosts(key);  
    }
}

function openModal(imageUrl) {
    if (imageUrl) {
        document.getElementById('modalImage').src = imageUrl;
        document.getElementById('myModal').style.display = 'flex';
    }
}

function closeModal() {
    document.getElementById('myModal').style.display = 'none';
}

(async () => {
    const key = await getKeyFromUrl() || await generateAndStoreKey();
    fetchPosts(key);
})();
