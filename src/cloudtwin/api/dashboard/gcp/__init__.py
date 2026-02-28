from cloudtwin.api.dashboard.gcp import (
    cloudfunctions,
    cloudtasks,
    firestore,
    pubsub,
    secretmanager,
    storage,
)

routers = [
    storage.router,
    pubsub.router,
    firestore.router,
    cloudtasks.router,
    secretmanager.router,
    cloudfunctions.router,
]
