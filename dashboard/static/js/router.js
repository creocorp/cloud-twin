// ─── Client-side router ───────────────────────────────────────────────────────
// Hash-based routing. Reads location.hash, shows the right page div,
// starts/stops polling, and keeps the sidebar nav-link active state in sync.
//
// Depends on: utils.js (startPoll / stopPoll / stopAllPolls / reinitIcons)
//             js/pages/*.js  (all loadXxx functions must be defined first)

/**
 * Route table: hash path → { page element id, load function, poll interval ms }
 * poll: null means load once on navigation; a number enables auto-refresh.
 */
const ROUTES = {
  '/':                 { page: 'page-overview',         load: loadOverview,   poll: 10000 },
  '/events':           { page: 'page-events',           load: loadEvents,     poll: 3000  },
  '/aws/ses':          { page: 'page-aws-ses',          load: loadSES,        poll: null  },
  '/aws/s3':           { page: 'page-aws-s3',           load: loadS3,         poll: null  },
  '/aws/sns':          { page: 'page-aws-sns',          load: loadSNS,        poll: null  },
  '/aws/sqs':          { page: 'page-aws-sqs',          load: loadSQS,        poll: null  },
  '/aws/bedrock':          { page: 'page-aws-bedrock',          load: loadBedrock,          poll: null  },
  '/aws/dynamodb':          { page: 'page-aws-dynamodb',         load: loadDynamoDB,         poll: null  },
  '/aws/lambda':            { page: 'page-aws-lambda',           load: loadLambda,           poll: null  },
  '/aws/secretsmanager':    { page: 'page-aws-secretsmanager',   load: loadSecretsManager,   poll: null  },
  '/azure/blob':            { page: 'page-azure-blob',           load: loadAzureBlob,        poll: null  },
  '/azure/servicebus':      { page: 'page-azure-servicebus',     load: loadAzureSB,          poll: null  },
  '/azure/eventgrid':       { page: 'page-azure-eventgrid',      load: loadAzureEventGrid,   poll: null  },
  '/azure/functions':       { page: 'page-azure-functions',      load: loadAzureFunctions,   poll: null  },
  '/azure/keyvault':        { page: 'page-azure-keyvault',       load: loadAzureKeyVault,    poll: null  },
  '/azure/queue':           { page: 'page-azure-queue',          load: loadAzureQueue,       poll: null  },
  '/gcp/storage':           { page: 'page-gcp-storage',          load: loadGCSStorage,       poll: null  },
  '/gcp/pubsub':            { page: 'page-gcp-pubsub',           load: loadPubSub,           poll: null  },
  '/gcp/cloudfunctions':    { page: 'page-gcp-cloudfunctions',   load: loadCloudFunctions,   poll: null  },
  '/gcp/cloudtasks':        { page: 'page-gcp-cloudtasks',       load: loadCloudTasks,       poll: null  },
  '/gcp/firestore':         { page: 'page-gcp-firestore',        load: loadFirestore,        poll: null  },
  '/gcp/secretmanager':     { page: 'page-gcp-secretmanager',    load: loadGcpSecretManager, poll: null  },
};

function navigate() {
  const route = location.hash.slice(1) || '/';
  const entry = ROUTES[route];

  if (!entry) {
    location.hash = '#/';
    return;
  }

  stopAllPolls();

  // Sync sidebar active state
  document.querySelectorAll('.nav-link').forEach(el => {
    el.classList.toggle('active', el.dataset.route === route);
  });

  // Swap visible page
  document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
  document.getElementById(entry.page).classList.add('active');

  if (entry.poll) {
    startPoll('page', entry.load, entry.poll);
  } else {
    entry.load();
  }
}

window.addEventListener('hashchange', navigate);
