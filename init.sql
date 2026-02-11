-- 1. 开启 pg_trgm 扩展，用于中文语义和相似度搜索
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- 2. 标准词根表
CREATE TABLE standard_word_roots (
    id SERIAL PRIMARY KEY,
    cn_name VARCHAR(100) NOT NULL,              -- 中文名称 (如：金额)
    en_abbr VARCHAR(50) NOT NULL UNIQUE,        -- 英文缩写 (如：amt)
    en_full_name VARCHAR(100),                  -- 英文全称 (如：amount)
    associated_terms TEXT,                      -- 同义词/关联词 (如：钱,费用,价格)
    remark TEXT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- 在关联词上创建 GIN 索引，支撑高性能模糊搜索
CREATE INDEX idx_roots_associated_terms_gin ON standard_word_roots USING GIN (associated_terms gin_trgm_ops);

-- 3. 标准字段库
CREATE TABLE standard_fields (
    id SERIAL PRIMARY KEY,
    field_cn_name VARCHAR(200) NOT NULL,        -- 字段中文全称 (如：订单支付金额)
    field_en_name VARCHAR(200) NOT NULL,        -- 自动生成的英文名 (如：order_pay_amt)
    composition_ids INT[],                      -- 关联的词根ID链 [1, 5, 22]
    data_type VARCHAR(50),                      -- 推荐数据类型
    is_standard BOOLEAN DEFAULT FALSE,          -- 是否经管理员审核为标准
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- 4. 任务提醒表 (小红点来源)
CREATE TABLE notification_tasks (
    id SERIAL PRIMARY KEY,
    task_type VARCHAR(50) NOT NULL,             -- ROOT_REQUEST / FIELD_UPDATE
    payload JSONB,                              -- 存储用户请求详情
    is_read BOOLEAN DEFAULT FALSE,              -- 核心：已读未读
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- 增加标准字段的同义词/关联词字段
ALTER TABLE standard_fields ADD COLUMN associated_terms TEXT;
-- 创建 GIN 索引加速搜索
CREATE INDEX idx_fields_associated_terms_gin ON standard_fields USING GIN (associated_terms gin_trgm_ops);


CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    username VARCHAR(50) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    role VARCHAR(20) DEFAULT 'user', -- 'admin' 或 'user'
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- 插入默认管理员 (密码为 admin)
INSERT INTO users (username, password_hash, role) 
VALUES ('admin', '$argon2id$v=19$m=19456,t=2,p=1$pL1X6nNwXH/Qz777puvtwA$PfoXdJLSEdkEc/Y10iM+GcsMZiFa3y5P5ynDVN+BcXI', 'admin')
ON CONFLICT (username) DO NOTHING;

-- 确保标准字段也有同义词索引
CREATE INDEX IF NOT EXISTS idx_fields_associated_terms_trgm ON standard_fields USING GIN (associated_terms gin_trgm_ops);

-- 给标准中文名增加唯一约束
ALTER TABLE standard_fields ADD CONSTRAINT unique_field_cn_name UNIQUE (field_cn_name);

-- 给标准英文名增加唯一约束
ALTER TABLE standard_fields ADD CONSTRAINT unique_field_en_name UNIQUE (field_en_name);