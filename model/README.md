https://huggingface.co/Xenova/paraphrase-multilingual-MiniLM-L12-v2/tree/main

下载以下文件：
model.onnx
config.json
tokenizer.json
tokenizer_config.json
special_tokens_map.json

在 Rust 代码中指定缓存路径

let model = TextEmbedding::try_new(
    InitOptions::new(EmbeddingModel::ParaphraseMLMiniLML12V2)
        .with_cache_dir(PathBuf::from("./model_cache")) // 指定项目根目录下的 model_cache
        .with_show_download_progress(true)
).expect("Failed to load embedding model");

然后将下载的文件按照 HuggingFace 的目录结构放入 ./model_cache 文件夹