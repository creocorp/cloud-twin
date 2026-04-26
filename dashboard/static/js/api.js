// ─── Dashboard API client ─────────────────────────────────────────────────────
// Thin fetch wrappers for every /api/dashboard/* endpoint.
// Depends on: nothing (pure fetch).

const BASE = '/api/dashboard';

async function get(path) {
  const r = await fetch(`${BASE}${path}`);
  if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
  return r.json();
}

const api = {
  health:  ()        => get('/health'),
  events:  (n = 100) => get(`/events?limit=${n}`),

  aws: {
    ses:            () => get('/aws/ses'),
    s3:             () => get('/aws/s3'),
    sns:            () => get('/aws/sns'),
    sqs:            () => get('/aws/sqs'),
    bedrock:        () => get('/aws/bedrock'),
    dynamodb:       () => get('/aws/dynamodb'),
    lambda:         () => get('/aws/lambda'),
    secretsmanager: () => get('/aws/secretsmanager'),
  },

  azure: {
    blob:       () => get('/azure/blob'),
    serviceBus: () => get('/azure/servicebus'),
    eventGrid:  () => get('/azure/eventgrid'),
    functions:  () => get('/azure/functions'),
    keyVault:   () => get('/azure/keyvault'),
    queue:      () => get('/azure/queue'),
  },

  gcp: {
    storage:        () => get('/gcp/storage'),
    pubsub:         () => get('/gcp/pubsub'),
    cloudFunctions: () => get('/gcp/cloudfunctions'),
    cloudTasks:     () => get('/gcp/cloudtasks'),
    firestore:      () => get('/gcp/firestore'),
    secretManager:  () => get('/gcp/secretmanager'),
  },
};
