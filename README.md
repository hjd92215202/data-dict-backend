# 数据字典后端 (data-dict-backend)

简体中文说明：本仓库为“数据标准管理系统”的后端服务，使用 Rust + Axum 实现，提供字段标准化、词根管理、分词建议与基于向量的语义检索功能，结合 Qdrant 作为向量存储/检索引擎。

## 快速开始
- 克隆代码并设置环境变量：

```bash
export DATABASE_URL=postgres://user:pass@localhost:5432/dbname
export QDRANT_URL=http://localhost:6334
export RUST_LOG=info
cargo run --release
```

在 Windows PowerShell 下请使用 `set` / `setx` 或通过 IDE 配置环境变量。

## 项目结构（概要）
- **入口**: [src/main.rs](src/main.rs#L1)
- **路由与处理器**: [src/handlers](src/handlers/mod.rs#L1)
- **业务逻辑（服务层）**: [src/services](src/services/mod.rs#L1)
- **数据模型**: [src/models](src/models/mod.rs#L1) （示例文件： [src/models/field.rs](src/models/field.rs#L1), [src/models/user.rs](src/models/user.rs#L1)）
- **中间件**: [src/middleware/auth.rs](src/middleware/auth.rs#L1)
- **向量库配置**: [qdrant/config.yaml](qdrant/config.yaml#L1)
- **离线嵌入模型缓存**: `model/fastembed_cache`（内网离线模型存放）

## 架构概览
- 使用 `axum` 作为 Web 框架，`tokio` 异步运行时。
- 业务分层：Handler (HTTP 层) → Service (业务逻辑) → Models (数据库实体/映射)。
- 持久层使用 Postgres（通过 `sqlx`）；向量检索使用 Qdrant（通过 `qdrant-client`）；文本向量化使用 `fastembed` 离线模型。
- 分词使用 `jieba-rs`，系统在启动时会把标准词根与同义词注入到分词器以提升匹配质量。

## 核心模块与关键逻辑

**1. 启动与初始化 (src/main.rs):**
- 初始化日志与环境、建立 Postgres 连接池（`PgPool`）。
- 加载并封装离线向量嵌入模型（`fastembed::TextEmbedding`），并放入全局 `AppState`。
- 初始化/创建 Qdrant Collection（`word_roots` 与 `standard_fields`），并执行冷启动同步：
  - `sync_roots_to_qdrant()`：读取标准词根并生成向量上传到 Qdrant。
  - `sync_fields_to_qdrant()`：读取标准字段并生成向量上传到 Qdrant。

**2. 路由分层与权限 (src/main.rs / src/middleware/auth.rs):**
- 路由分为 `/api/auth`、`/api/public`、`/api/admin` 三层，管理员路由通过中间件 `guard` 验证 JWT 且角色为 `admin`。

**3. 分词建议 (Handlers → Services):**
- 接口：`/api/admin/suggest` 由 [src/handlers/mapping_handler.rs](src/handlers/mapping_handler.rs) 暴露。
- 关键 Service 方法：`mapping_service::suggest_field_name(pool, input)`：
  - 优先尝试整句（全称）精确或同义词匹配（SQL ILIKE 检索）。
  - 若未命中，使用 `jieba` 精准模式切分后，对每个分词进行词根匹配，返回 `Segment` 列表。

**4. 语义相似度检索 (Handlers → Qdrant):**
- 接口：`/api/public/similar-roots`，处理流程：
  - 使用全局嵌入模型对输入文本进行向量化（通过 `state.embed_model.lock()` 获取模型实例）。
  - 调用 Qdrant 的检索接口（`search_points`）从 `word_roots` 集合召回相似项，并返回带分数与 payload 的结果。

**5. 用户与权限管理 (src/handlers/auth_handler.rs, src/models/user.rs):**
- 使用 `argon2` 进行密码哈希。系统启动时会保证存在默认管理员 `admin/admin`（见 `ensure_default_admin`）。
- JWT 使用 `jsonwebtoken` 进行签发/验证；中间件从 `Authorization: Bearer <token>` 读取并校验角色。

**6. 数据模型（示例）**
- `StandardField`（[src/models/field.rs](src/models/field.rs)）：字段实体，包含 `composition_ids: Vec<i32>`（关联词根），`associated_terms` 等。
- `User`（[src/models/user.rs](src/models/user.rs)）：用户实体与 `Claims`（用于 JWT）。

## 重要函数与位置（快速索引）
- 启动入口与全局状态： [src/main.rs](src/main.rs#L235)
  - `main()`：服务启动、模型加载、Qdrant 初始化、数据冷启动同步（[src/main.rs](src/main.rs#L235)）。
  - `sync_roots_to_qdrant()`：词根向量同步（[src/main.rs](src/main.rs#L95)）。
  - `sync_fields_to_qdrant()`：标准字段向量同步（[src/main.rs](src/main.rs#L145)）。
  - `ensure_default_admin()`：确保默认 admin 账户存在（[src/main.rs](src/main.rs#L66)）。
- 分词建议：
  - Handler: [src/handlers/mapping_handler.rs](src/handlers/mapping_handler.rs#L34)::`suggest_mapping`
  - Service: [src/services/mapping_service.rs](src/services/mapping_service.rs#L11)::`suggest_field_name`
- 语义检索：
  - Handler: [src/handlers/mapping_handler.rs](src/handlers/mapping_handler.rs#L53)::`search_similar_roots`
  - Qdrant 调用：使用 `qdrant-client` 的 `search_points` / `upsert_points`。
- 中间件权限守卫： [src/middleware/auth.rs](src/middleware/auth.rs#L14)::`guard`
- 字段相关：
  - 创建字段: [src/handlers/field_handler.rs](src/handlers/field_handler.rs#L14)::`create_field`
  - 用户搜索接口: [src/handlers/field_handler.rs](src/handlers/field_handler.rs#L224)::`search_field`
- 词根相关：
  - 创建词根: [src/handlers/word_root_handler.rs](src/handlers/word_root_handler.rs#L51)::`create_root`
  - 批量导入: [src/handlers/word_root_handler.rs](src/handlers/word_root_handler.rs#L121)::`batch_create_roots`
- 任务相关：
  - 用户提交任务: [src/handlers/task_handler.rs](src/handlers/task_handler.rs#L21)::`submit_task`
  - 管理员待办列表: [src/handlers/task_handler.rs](src/handlers/task_handler.rs#L45)::`list_tasks`
  - 完成任务: [src/handlers/task_handler.rs](src/handlers/task_handler.rs#L61)::`complete_task`
  - 待处理任务计数: [src/handlers/task_handler.rs](src/handlers/task_handler.rs#L75)::`count_unprocessed_tasks`

## 配置项与外部依赖
- 必须环境变量：`DATABASE_URL`（Postgres）、`QDRANT_URL`（可选，默认 http://localhost:6334）、`RUST_LOG`。
- Qdrant 建议配置见： [qdrant/config.yaml](qdrant/config.yaml)（包含 on_disk_payload、memmap 与 WAL 优化项）。

## 开发注意与建议
- 嵌入模型为离线缓存机制：请保证 `model/fastembed_cache` 路径与子目录名称与仓库中一致，以便离线加载。
- 嵌入计算使用全局 Mutex 包裹模型实例（同步锁），注意避免在持锁状态下做耗时 IO；目前代码通过在短代码块内锁住模型来降低阻塞。
- Qdrant `on_disk_payload: true` 可显著降低内存占用，适合大规模 metadata 场景。

## 常见命令
```bash
# 运行（开发）
cargo run

# 构建发布版
cargo build --release
```

## 参考文件
- 入口: [src/main.rs](src/main.rs)
- 路由与 Handler: [src/handlers/mod.rs](src/handlers/mod.rs)
- Service 示例: [src/services/mapping_service.rs](src/services/mapping_service.rs)
- 模型示例: [src/models/field.rs](src/models/field.rs)
- 中间件: [src/middleware/auth.rs](src/middleware/auth.rs)
- Qdrant 配置: [qdrant/config.yaml](qdrant/config.yaml)

---

