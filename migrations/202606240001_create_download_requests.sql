CREATE TABLE IF NOT EXISTS download_requests (
    id UUID PRIMARY KEY,
    provider TEXT NOT NULL,
    source_url TEXT NOT NULL,
    normalized_url TEXT NOT NULL,
    source_host TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS download_requests_provider_created_at_idx
    ON download_requests (provider, created_at DESC);

CREATE INDEX IF NOT EXISTS download_requests_source_host_idx
    ON download_requests (source_host);
