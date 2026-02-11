# API Documentation (summary)

This document lists the main HTTP endpoints exposed by the backend, method, purpose, brief request/response shape, and authorization requirement.

Base path: `/api`

## /api/auth (public)
- POST /signup
  - Description: user self-registration.
  - Body: { "username": string, "password": string }
  - Response: 201 Created or 400 on failure.
  - Auth: none

- POST /login
  - Description: user login, returns JWT.
  - Body: { "username": string, "password": string }
  - Response: 200 { "token": string, "role": string } or 401
  - Auth: none

## /api/public
- GET /health
  - Description: health check. Returns DB connectivity status.
  - Response: 200 { status: "up", database: "connected" } or 503
  - Auth: none

- GET /search?q=...
  - Description: search standard fields by text (first SQL fuzzy search, fallback to vector search).
  - Query: `q` string
  - Response: 200 list of field objects or empty list
  - Auth: none

- POST /tasks
  - Description: submit a field request (user-submitted task).
  - Body: { "field_cn_name": string }
  - Response: 201 Created or 500
  - Auth: none

- GET /similar-roots?q=...
  - Description: semantic nearest-neighbors from `word_roots` via embedding → Qdrant search.
  - Query: `q` string
  - Response: 200 list of suggestions [{ id, cn_name, en_abbr, score }]
  - Auth: none

## /api/admin (requires JWT role=admin)

### Word roots
- POST /roots
  - Create a single word root.
  - Body: CreateWordRoot (cn_name, en_abbr, en_full_name, associated_terms, remark)
  - Response: 201 with created root

- POST /roots/batch
  - Batch import many word roots: Body { items: [CreateWordRoot] }
  - Response: 200 with ImportResult { success_count, failure_count, errors }

- GET /roots
  - List/paginate word roots. Query params: page, page_size, q
  - Response: { items: [...], total }

- PUT /roots/:id
  - Update a word root by id. Body: CreateWordRoot

- DELETE /roots/:id
  - Delete by id (also removes vector in Qdrant)

- DELETE /roots/clear
  - Truncate all word roots and clear Qdrant `word_roots` collection

### Standard fields
- POST /fields
  - Create standard field: Body CreateFieldRequest (field_cn_name, field_en_name, composition_ids: [i32], data_type?, associated_terms?)
  - Response: 201 with created StandardField

- GET /fields
  - Paginated list, query: page, page_size, q

- GET /fields/:id
  - Returns composition (word-root) details for the field

- PUT /fields/:id
  - Update field (body same as CreateFieldRequest)

- DELETE /fields/:id
  - Delete field and remove vector from Qdrant

- DELETE /fields/clear
  - Truncate standard_fields and clear Qdrant `standard_fields` collection

### Users
- POST /users
  - Admin creates user. Body: { username, password, role }
  - Response: 201

- GET /users
  - List all users (admin)

- PUT /users/:id
  - Update user's role. Body: { role }

- DELETE /users/:id
  - Delete user

### Tasks (admin)
- GET /tasks
  - List pending notification tasks (is_read=false)

- PUT /tasks/:id
  - Mark task complete (set is_read=true)

- GET /tasks/count
  - Return { count: number } of unprocessed tasks

## Notes & Behavior
- Embedding: endpoints that add or update word roots / fields will compute an embedding (via `fastembed` model) and upsert a point to Qdrant with payloads like `{ cn_name, en_abbr/en_name }`.
- Search behavior: text search uses SQL ILIKE first; if no results, the API falls back to vector search in Qdrant.
- Auth: admin routes are protected by JWT in `Authorization: Bearer <token>`; the middleware validates JWT and requires `role == "admin"`.

If you want full example requests/responses or an OpenAPI 3.0 YAML generated from these handlers, I can produce it next.

## Examples (curl + JSON)

Note: replace `http://localhost:3000` with your server address and `TOKEN` with a valid JWT for admin endpoints.

-- Public / Auth

- Signup

```bash
curl -X POST http://localhost:3000/api/auth/signup \
  -H "Content-Type: application/json" \
  -d '{"username":"alice","password":"s3cret"}'
```

Response: 201 Created

- Login

```bash
curl -X POST http://localhost:3000/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"alice","password":"s3cret"}'
```

Response: 200

```json
{ "token": "ey...", "role": "user" }
```

- Health

```bash
curl http://localhost:3000/api/public/health
```

Response: 200

```json
{ "status": "up", "database": "connected" }
```

- Search fields (fallback to vector search)

```bash
curl "http://localhost:3000/api/public/search?q=客户名称"
```

Response: 200

```json
[ { "id": 12, "field_cn_name": "客户名称", "field_en_name": "customer_name", "composition_ids": [1,2], "score": null } ]
```

- Semantic similar roots

```bash
curl "http://localhost:3000/api/public/similar-roots?q=订单金额"
```

Response: 200

```json
[ { "id": "3", "cn_name": "金额", "en_abbr": "amt", "score": 0.92 } ]
```

-- Tasks

- Submit user task

```bash
curl -X POST http://localhost:3000/api/public/tasks \
  -H "Content-Type: application/json" \
  -d '{"field_cn_name":"新字段示例"}'
```

Response: 201 Created

-- Admin (requires `Authorization: Bearer TOKEN`)

- Create a word root

```bash
curl -X POST http://localhost:3000/api/admin/roots \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"cn_name":"订单号","en_abbr":"order_no","en_full_name":"order_number","associated_terms":"单号 订单编号","remark":"业务唯一标识"}'
```

Response: 201

```json
{ "id": 42, "cn_name": "订单号", "en_abbr": "order_no", "associated_terms": "单号 订单编号" }
```

- Batch import word roots

```bash
curl -X POST http://localhost:3000/api/admin/roots/batch \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"items":[{"cn_name":"金额","en_abbr":"amt"},{"cn_name":"客户","en_abbr":"cust"}]}'
```

Response: 200

```json
{ "success_count": 2, "failure_count": 0, "errors": [] }
```

- List roots (paginated)

```bash
curl "http://localhost:3000/api/admin/roots?page=1&page_size=20&q=金额" \
  -H "Authorization: Bearer TOKEN"
```

Response: 200

```json
{ "items": [ { "id": 3, "cn_name": "金额" } ], "total": 1 }
```

- Create standard field

```bash
curl -X POST http://localhost:3000/api/admin/fields \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"field_cn_name":"交易金额","field_en_name":"transaction_amount","composition_ids":[3],"data_type":"decimal","associated_terms":"金额 支付金额"}'
```

Response: 201

```json
{ "id": 10, "field_cn_name": "交易金额", "field_en_name": "transaction_amount" }
```

- List fields (admin)

```bash
curl "http://localhost:3000/api/admin/fields?page=1&page_size=10&q=金额" \
  -H "Authorization: Bearer TOKEN"
```

Response: 200

```json
{ "items": [ { "id": 10, "field_cn_name": "交易金额" } ], "total": 1 }
```

- Get field composition (roots)

```bash
curl "http://localhost:3000/api/admin/fields/10" \
  -H "Authorization: Bearer TOKEN"
```

Response: 200

```json
[ { "id": 3, "cn_name": "金额", "en_abbr": "amt" } ]
```

- Update / Delete field

```bash
curl -X PUT http://localhost:3000/api/admin/fields/10 \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"field_cn_name":"交易金额(更新)","field_en_name":"transaction_amount","composition_ids":[3],"data_type":"decimal"}'

curl -X DELETE http://localhost:3000/api/admin/fields/10 \
  -H "Authorization: Bearer TOKEN"
```

Responses: 200 / 204 respectively

- User management (admin)

Create user:

```bash
curl -X POST http://localhost:3000/api/admin/users \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"username":"bob","password":"pwd","role":"user"}'
```

List users:

```bash
curl http://localhost:3000/api/admin/users -H "Authorization: Bearer TOKEN"
```

Update role:

```bash
curl -X PUT http://localhost:3000/api/admin/users/5 \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"role":"admin"}'
```

Delete user:

```bash
curl -X DELETE http://localhost:3000/api/admin/users/5 -H "Authorization: Bearer TOKEN"
```

- Tasks (admin)

List pending:

```bash
curl http://localhost:3000/api/admin/tasks -H "Authorization: Bearer TOKEN"
```

Complete a task:

```bash
curl -X PUT http://localhost:3000/api/admin/tasks/7 -H "Authorization: Bearer TOKEN"
```

Count unprocessed:

```bash
curl http://localhost:3000/api/admin/tasks/count -H "Authorization: Bearer TOKEN"
```

Response example:

```json
{ "count": 4 }
```
