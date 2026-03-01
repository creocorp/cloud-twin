"""GCP Firestore — HTTP handlers (REST/JSON)."""

from __future__ import annotations

import json
import logging

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse, Response

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.gcp.firestore.service import FirestoreService

log = logging.getLogger("cloudtwin.gcp.firestore")


def _doc_to_dict(doc) -> dict:
    return {
        "name": f"projects/{doc.project}/databases/(default)/documents/{doc.collection}/{doc.document_id}",
        "fields": json.loads(doc.fields),
        "createTime": doc.created_at,
    }


def make_router(service: FirestoreService) -> APIRouter:
    router = APIRouter()

    @router.patch(
        "/v1/projects/{project}/databases/(default)/documents/{collection}/{document_id}"
    )
    async def set_document(
        project: str, collection: str, document_id: str, request: Request
    ) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            body = {}
        fields = body.get("fields", body)
        try:
            doc = await service.set_document(project, collection, document_id, fields)
            return JSONResponse(_doc_to_dict(doc))
        except CloudTwinError as exc:
            return JSONResponse({"error": exc.message}, status_code=exc.http_status)

    @router.get(
        "/v1/projects/{project}/databases/(default)/documents/{collection}/{document_id}"
    )
    async def get_document(
        project: str, collection: str, document_id: str
    ) -> JSONResponse:
        try:
            doc = await service.get_document(project, collection, document_id)
            return JSONResponse(_doc_to_dict(doc))
        except NotFoundError as exc:
            return JSONResponse({"error": exc.message}, status_code=404)

    @router.delete(
        "/v1/projects/{project}/databases/(default)/documents/{collection}/{document_id}"
    )
    async def delete_document(
        project: str, collection: str, document_id: str
    ) -> Response:
        await service.delete_document(project, collection, document_id)
        return Response(status_code=200)

    @router.get("/v1/projects/{project}/databases/(default)/documents/{collection}")
    async def list_documents(project: str, collection: str) -> JSONResponse:
        docs = await service.list_documents(project, collection)
        return JSONResponse({"documents": [_doc_to_dict(d) for d in docs]})

    @router.post(
        "/v1/projects/{project}/databases/(default)/documents/{collection}:runQuery"
    )
    async def run_query(
        project: str, collection: str, request: Request
    ) -> JSONResponse:
        try:
            body = await request.json()
        except Exception:
            body = {}
        where = body.get("structuredQuery", {}).get("where", {})
        field = where.get("fieldFilter", {}).get("field", {}).get("fieldPath", "")
        op = where.get("fieldFilter", {}).get("op", "==")
        value = str(
            where.get("fieldFilter", {}).get("value", {}).get("stringValue", "")
        )
        docs = await service.query(project, collection, field, op, value)
        return JSONResponse({"documents": [_doc_to_dict(d) for d in docs]})

    return router
