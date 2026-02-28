from cloudtwin.api.dashboard.azure import (
    blob,
    eventgrid,
    functions,
    keyvault,
    queue,
    servicebus,
)

routers = [
    blob.router,
    servicebus.router,
    queue.router,
    eventgrid.router,
    keyvault.router,
    functions.router,
]
