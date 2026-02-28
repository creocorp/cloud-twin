"""Dashboard — /api/dashboard/gcp/firestore"""

from __future__ import annotations

from fastapi import APIRouter, Request

router = APIRouter()


@router.get("/gcp/firestore")
async def gcp_firestore(request: Request):
    config = request.app.state.config
    repos = request.app.state.repos
    project = config.providers.gcp.project
    documents = await repos["firestore_document"].list_by_project(project)
    collections: dict[str, int] = {}
    for d in documents:
        collections[d.collection] = collections.get(d.collection, 0) + 1
    return {
        "collections": [
            {"name": name, "document_count": count}
            for name, count in sorted(collections.items())
        ]
    }
