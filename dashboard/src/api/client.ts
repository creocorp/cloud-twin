/**
 * CloudTwin Dashboard API client.
 *
 * Calls the /api/dashboard/* endpoints served by the FastAPI backend.
 * The Python agent is responsible for implementing these endpoints;
 * this file defines the expected response shapes and fetch helpers.
 */

const BASE = "/api/dashboard";

async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`);
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(`${res.status} ${text}`);
  }
  return res.json() as Promise<T>;
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

export interface TelemetryEvent {
  id: number;
  provider: string;
  service: string;
  action: string;
  payload: Record<string, unknown>;
  created_at: string;
}

export interface HealthResponse {
  status: "ok" | "degraded";
  storage_mode: string;
  services: Record<string, boolean>;
}

// ---------------------------------------------------------------------------
// AWS — SES
// ---------------------------------------------------------------------------

export interface SesIdentity {
  identity: string;
  type: "email" | "domain";
  verified: boolean;
  created_at: string;
}

export interface SesMessage {
  id: number;
  source: string;
  destination: string;
  subject: string;
  created_at: string;
}

export interface SesData {
  identities: SesIdentity[];
  messages: SesMessage[];
}

// ---------------------------------------------------------------------------
// AWS — S3
// ---------------------------------------------------------------------------

export interface S3Bucket {
  name: string;
  created_at: string;
}

export interface S3Object {
  key: string;
  size: number;
  content_type: string;
  created_at: string;
}

export interface S3Data {
  buckets: S3Bucket[];
}

// ---------------------------------------------------------------------------
// AWS — SNS
// ---------------------------------------------------------------------------

export interface SnsTopic {
  arn: string;
  name: string;
  created_at: string;
}

export interface SnsSubscription {
  arn: string;
  topic_arn: string;
  protocol: string;
  endpoint: string;
}

export interface SnsData {
  topics: SnsTopic[];
  subscriptions: SnsSubscription[];
}

// ---------------------------------------------------------------------------
// AWS — SQS
// ---------------------------------------------------------------------------

export interface SqsQueue {
  name: string;
  url: string;
  message_count: number;
  created_at: string;
}

export interface SqsData {
  queues: SqsQueue[];
}

// ---------------------------------------------------------------------------
// Azure — Blob Storage
// ---------------------------------------------------------------------------

export interface AzureContainer {
  name: string;
  blob_count: number;
  created_at: string;
}

export interface AzureBlobData {
  containers: AzureContainer[];
}

// ---------------------------------------------------------------------------
// Azure — Service Bus
// ---------------------------------------------------------------------------

export interface AsbQueue {
  name: string;
  message_count: number;
  created_at: string;
}

export interface AsbTopic {
  name: string;
  subscription_count: number;
  created_at: string;
}

export interface AzureServiceBusData {
  queues: AsbQueue[];
  topics: AsbTopic[];
}

// ---------------------------------------------------------------------------
// GCP — Cloud Storage
// ---------------------------------------------------------------------------

export interface GcsBucket {
  name: string;
  location: string;
  object_count: number;
  created_at: string;
}

export interface GcsData {
  buckets: GcsBucket[];
}

// ---------------------------------------------------------------------------
// GCP — Pub/Sub
// ---------------------------------------------------------------------------

export interface PubsubTopic {
  name: string;
  subscription_count: number;
  created_at: string;
}

export interface PubsubSubscription {
  name: string;
  topic: string;
  message_count: number;
  created_at: string;
}

export interface PubsubData {
  topics: PubsubTopic[];
  subscriptions: PubsubSubscription[];
}

// ---------------------------------------------------------------------------
// API functions
// ---------------------------------------------------------------------------

export const api = {
  health: () => get<HealthResponse>("/health"),
  events: (limit = 50) => get<{ events: TelemetryEvent[] }>(`/events?limit=${limit}`),

  aws: {
    ses: () => get<SesData>("/aws/ses"),
    s3: () => get<S3Data>("/aws/s3"),
    sns: () => get<SnsData>("/aws/sns"),
    sqs: () => get<SqsData>("/aws/sqs"),
  },

  azure: {
    blob: () => get<AzureBlobData>("/azure/blob"),
    serviceBus: () => get<AzureServiceBusData>("/azure/servicebus"),
  },

  gcp: {
    storage: () => get<GcsData>("/gcp/storage"),
    pubsub: () => get<PubsubData>("/gcp/pubsub"),
  },
};
