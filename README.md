# HushNet Registry

Centralized registry server for the HushNet decentralized network, enabling node registration, discovery, and monitoring.

## Description

HushNet Registry is a Rust-based backend service that maintains a registry of active nodes in the HushNet network. It provides a REST API for secure node registration using Ed25519 cryptographic signatures, along with health monitoring and geolocation features.

## Architecture

### Technologies

- **Rust** (Edition 2021)
- **Axum** - Async web framework
- **PostgreSQL** - Relational database
- **Docker** - Containerization
- **SQLx** - Async PostgreSQL client
- **Ed25519-Dalek** - Elliptic curve cryptography
- **MaxMindDB** - IP geolocation
- **Tokio** - Async runtime

### Core Components

1. **REST API** - HTTP interface for nodes and clients
2. **Authentication System** - Challenge-response with Ed25519 signatures
3. **Health Worker** - Periodic node monitoring
4. **PostgreSQL Database** - Persistent storage

## Installation

### Prerequisites

- Docker and Docker Compose
- Rust 1.70+ (for local development)
- PostgreSQL 15+ (for local development)

### Docker Deployment

1. Clone the repository:

```bash
git clone https://github.com/HushNet/HushNet-Registry.git
cd HushNet-Registry
```

2. Configure environment variables (optional):

```bash
# Create a .env file
POSTGRES_USER=postgres
POSTGRES_PASSWORD=dev
POSTGRES_DB=hushreg
POSTGRES_PORT=5432
REGISTRY_PORT=8081
HEALTH_TIMEOUT_MS=3000
```

3. Start the services:

```bash
docker compose up --build
```

The registry will be accessible at `http://localhost:8081`

### Local Development

1. Install dependencies:

```bash
cargo build
```

2. Set up the database:

```bash
# Start PostgreSQL
docker run -d \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=dev \
  -e POSTGRES_DB=hushreg \
  -p 5432:5432 \
  postgres:latest

# Apply schema
psql -h localhost -U postgres -d hushreg -f sql_models/seed.sql
```

3. Download GeoLite2 database:

```bash
bash setup_geolite.sh
```

4. Configure DATABASE_URL:

```bash
export DATABASE_URL="postgres://postgres:dev@localhost:5432/hushreg"
```

5. Run the application:

```bash
cargo run
```

## API Reference

### Endpoints

#### POST /api/registry/challenge

Request a cryptographic challenge for authentication.

**Request:**

```json
{
  "pubkey_b64": "base64_encoded_public_key"
}
```

**Response:**

```json
{
  "nonce": "random_nonce",
  "expires_at": "2025-11-07T12:34:56Z"
}
```

**Status Codes:**

- 200: Challenge generated successfully
- 400: Invalid or missing public key

#### POST /api/registry/register

Register a new node in the registry.

**Request:**

```json
{
  "payload": {
    "name": "My Node",
    "host": "node.example.com",
    "api_base_url": "https://node.example.com/api",
    "protocol_version": "1.0",
    "features": {},
    "contact_email": "admin@example.com"
  },
  "nonce": "challenge_nonce",
  "signature_b64": "base64_encoded_signature",
  "pubkey_b64": "base64_encoded_public_key"
}
```

**Response:**

```json
{
  "ok": true
}
```

**Status Codes:**

- 200: Registration successful
- 400: Invalid data or expired nonce
- 401: Invalid signature
- 403: Host already registered with another key

**Notes:**

- The payload is canonicalized before signing
- Signature must be computed on: `canonical_json(payload) + nonce`
- Nonce expires after 5 minutes

#### POST /api/registry/heartbeat

Update an existing node's status.

**Request:**

```json
{
  "host": "node.example.com",
  "nonce": "random_nonce",
  "signature_b64": "base64_encoded_signature",
  "pubkey_b64": "base64_encoded_public_key"
}
```

**Response:**

```json
{
  "ok": true
}
```

**Status Codes:**

- 200: Heartbeat recorded
- 400: Invalid data
- 401: Invalid signature

**Notes:**

- Signature must be computed on: `host + nonce`

#### GET /api/nodes

Retrieve the list of all registered nodes.

**Response:**

```json
{
  "nodes": [
    {
      "name": "My Node",
      "host": "node.example.com",
      "ip": "192.168.1.1",
      "api_base_url": "https://node.example.com/api",
      "protocol_version": "1.0",
      "features": {},
      "country_code": "FR",
      "country_name": "France",
      "last_seen_at": "2025-11-07T12:34:56Z",
      "last_latency_ms": 150,
      "status": "online"
    }
  ]
}
```

**Status Codes:**

- 200: List retrieved successfully

**Notes:**

- Nodes are sorted by status (online first) then by name
- Status can be: `online`, `offline`, or `unknown`

## Database Schema

### Table: nodes

Stores information about registered nodes.

| Column             | Type         | Description                                    |
|--------------------|--------------|------------------------------------------------|
| id                 | UUID         | Unique identifier (PK)                         |
| name               | TEXT         | Node name                                      |
| host               | TEXT         | Hostname (unique)                              |
| ip                 | INET         | Resolved IP address                            |
| api_base_url       | TEXT         | Base API URL                                   |
| pubkey             | BYTEA        | Ed25519 public key (unique)                    |
| protocol_version   | TEXT         | Protocol version                               |
| features           | JSONB        | Supported features                             |
| contact_email      | TEXT         | Contact email                                  |
| registered_at      | TIMESTAMPTZ  | Registration timestamp                         |
| country_code       | TEXT         | ISO country code (geolocation)                 |
| country_name       | TEXT         | Country name (geolocation)                     |
| last_seen_at       | TIMESTAMPTZ  | Last activity detected                         |
| last_latency_ms    | INTEGER      | Last measured latency (ms)                     |
| status             | TEXT         | Status: online/offline/unknown                 |
| uptime_ratio       | REAL         | Availability ratio                             |

### Table: challenges

Stores temporary authentication challenges.

| Column      | Type         | Description                          |
|-------------|--------------|--------------------------------------|
| nonce       | TEXT         | Unique nonce (PK)                    |
| pubkey_b64  | TEXT         | Base64-encoded public key            |
| expires_at  | TIMESTAMPTZ  | Expiration timestamp                 |

## Authentication Process

### Registration Flow

1. **Key Generation**: Node generates an Ed25519 key pair
2. **Challenge Request**: POST /api/registry/challenge with public key
3. **Payload Signing**: Node signs `canonical_json(payload) + nonce`
4. **Registration**: POST /api/registry/register with signed payload
5. **Verification**: Registry verifies signature and registers the node

### JSON Canonicalization

The payload is canonicalized before signing to ensure consistency:

- Object keys are sorted alphabetically
- The process is recursive for nested objects
- Arrays preserve their order

Example:

```json
// Original
{"z": 1, "a": 2}

// Canonicalized
{"a": 2, "z": 1}
```

## Health Monitoring

### Health Worker

The service runs a background worker that:

- Executes every 60 seconds
- Checks each node's `/health` endpoint
- Measures response latency
- Updates status and geolocation
- Configurable timeout (default: 3000ms)

### Node Status

- **online**: Node responded successfully to last check
- **offline**: Node failed to respond or returned an error
- **unknown**: Initial status, never checked

### Geolocation

The service uses the GeoLite2 City database to:

- Determine node country via IP address
- Store ISO country code (e.g., "FR", "US")
- Store country name (e.g., "France", "United States")

## Configuration

### Environment Variables

| Variable           | Description                          | Default   |
|--------------------|--------------------------------------|-----------|
| DATABASE_URL       | PostgreSQL connection URL            | Required  |
| POSTGRES_USER      | PostgreSQL user                      | postgres  |
| POSTGRES_PASSWORD  | PostgreSQL password                  | dev       |
| POSTGRES_DB        | Database name                        | hushreg   |
| POSTGRES_PORT      | PostgreSQL port                      | 5432      |
| REGISTRY_PORT      | Exposed registry port                | 8081      |
| HEALTH_TIMEOUT_MS  | Health check timeout (ms)            | 3000      |

### HTTP Middleware

- **CORS**: Permissive for all domains
- **Timeout**: 10 seconds per request
- **Tracing**: HTTP request logging
- **Compression**: Not enabled

## Security

### Cryptography

- **Algorithm**: Ed25519 (elliptic curve)
- **Encoding**: Standard Base64 for keys and signatures
- **Key Size**: 32 bytes
- **Signature Size**: 64 bytes

### Attack Protection

- **Single-use nonces**: Each challenge is deleted after use
- **Time expiration**: Challenges expire after 5 minutes
- **Host verification**: A host can only be registered with one public key
- **Cryptographic signatures**: All sensitive operations require valid signatures

### Best Practices

- Never share private keys
- Regenerate nonces for each operation
- Use HTTPS in production
- Restrict database access
- Monitor logs for intrusion attempts

## Project Structure

```
HushNet-Registry/
├── src/
│   ├── main.rs          # Entry point, API routes, handlers
│   ├── types.rs         # Data structures (Request/Response)
│   ├── canon.rs         # JSON canonicalization
│   └── mod.rs           # Module declarations
├── sql_models/
│   └── seed.sql         # Database schema
├── data/
│   └── GeoLite2-City.mmdb  # Geolocation database
├── Cargo.toml           # Rust dependencies
├── Dockerfile           # Multi-stage Docker image
├── docker-compose.yml   # Service orchestration
├── setup_geolite.sh     # GeoLite2 download script
└── README.md            # This documentation
```

## Troubleshooting

### Service fails to start

1. Check that PostgreSQL is accessible:

```bash
docker compose logs db
```

2. Verify environment variables:

```bash
docker compose config
```

3. Check registry logs:

```bash
docker compose logs registry
```

### Nodes appear as offline

1. Verify that the node's `/health` endpoint responds:

```bash
curl http://node.example.com/api/health
```

2. Increase timeout if nodes are slow:

```bash
HEALTH_TIMEOUT_MS=5000 docker compose up
```

### Signature errors

1. Verify that payload is canonicalized before signing
2. Verify that nonce is concatenated correctly
3. Verify that Base64 encoding is correct (standard, not URL-safe)

### Missing GeoLite2 database

```bash
bash setup_geolite.sh
```

## Performance

### Typical Metrics

- Response time: < 50ms for simple requests
- Throughput: > 1000 req/s on modern hardware
- Monitoring latency: 60 seconds between checks
- Memory: ~50MB in normal operation

### Optimizations

- Index on `status` for list queries
- UNIQUE constraints to prevent duplicates
- Prepared statements via SQLx
- Async Tokio runtime for concurrency

## Development

### Run tests

```bash
cargo test
```

### Check formatting

```bash
cargo fmt --check
```

### Lint code

```bash
cargo clippy
```

### Release build

```bash
cargo build --release
```

## Roadmap

- Prometheus metrics support
- WebSocket for real-time updates
- Web-based admin interface
- Advanced search API
- Full IPv6 support
- Multi-region replication

## License

See the LICENSE file in the repository.

## Contact

For questions or contributions, open an issue on GitHub.

## Contributors

Developed and maintained by the HushNet team.
