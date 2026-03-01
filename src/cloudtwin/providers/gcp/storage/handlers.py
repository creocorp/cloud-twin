"""
GCP Cloud Storage HTTP handlers – Google Storage JSON API v1.

Routes mounted at root (no prefix) to match the SDK's api_endpoint override.

  Buckets:
    POST   /storage/v1/b?project=       → create bucket
    GET    /storage/v1/b?project=       → list buckets
    GET    /storage/v1/b/{bucket}       → get bucket
    DELETE /storage/v1/b/{bucket}       → delete bucket

  Objects (simple upload):
    POST   /upload/storage/v1/b/{bucket}/o?uploadType=media&name={name}  → upload
    POST   /upload/storage/v1/b/{bucket}/o?uploadType=multipart           → upload (multipart)
    GET    /storage/v1/b/{bucket}/o                                       → list objects
    GET    /storage/v1/b/{bucket}/o/{name:path}                          → get metadata / download
    DELETE /storage/v1/b/{bucket}/o/{name:path}                          → delete
"""

from __future__ import annotations

import base64
import json
import logging
import re
import struct
import uuid
from urllib.parse import unquote

import google_crc32c
from fastapi import APIRouter, Query, Request, Response
from fastapi.responses import JSONResponse

from cloudtwin.core.errors import NotFoundError
from cloudtwin.providers.gcp.storage.service import StorageService

log = logging.getLogger("cloudtwin.gcp.storage")

# In-memory map of upload_id → {"bucket": ..., "name": ..., "content_type": ...}
# Used only for resumable uploads within a single process.
_pending_resumable: dict[str, dict] = {}


def _crc32c_b64(data: bytes | None) -> str:
    """Compute base64-encoded CRC32C of data (GCS checksum format)."""
    raw = data if data else b""
    checksum = google_crc32c.value(raw)
    return base64.b64encode(struct.pack(">I", checksum)).decode()


def _bucket_json(b) -> dict:
    return {
        "kind": "storage#bucket",
        "id": b.name,
        "name": b.name,
        "projectNumber": b.project,
        "location": b.location or "US",
        "timeCreated": b.created_at,
        "updated": b.created_at,
        "storageClass": "STANDARD",
        "etag": b.created_at,
        "selfLink": f"https://www.googleapis.com/storage/v1/b/{b.name}",
        "metageneration": "1",
        "iamConfiguration": {"uniformBucketLevelAccess": {"enabled": False}},
    }


def _object_json(o, bucket_name: str, base_url: str = "") -> dict:
    result = {
        "kind": "storage#object",
        "id": f"{bucket_name}/{o.name}/1",
        "name": o.name,
        "bucket": bucket_name,
        "contentType": o.content_type or "application/octet-stream",
        "size": str(o.content_length or 0),
        "timeCreated": o.created_at,
        "updated": o.created_at,
        "etag": o.created_at,
        "generation": "1",
        "metageneration": "1",
        "crc32c": _crc32c_b64(bytes(o.data) if o.data else b""),
    }
    # selfLink and mediaLink must point to the local server so the SDK uses
    # the correct base URL when downloading — if we have no base_url, omit them
    # to let the SDK fall back to API_BASE_URL from client_options.
    if base_url:
        base = base_url.rstrip("/")
        result["selfLink"] = f"{base}/storage/v1/b/{bucket_name}/o/{o.name}"
        result["mediaLink"] = (
            f"{base}/download/storage/v1/b/{bucket_name}/o/{o.name}?alt=media"
        )
    return result


def make_router(service: StorageService) -> APIRouter:
    router = APIRouter()

    # ------------------------------------------------------------------
    # Buckets
    # ------------------------------------------------------------------

    @router.post("/storage/v1/b")
    async def create_bucket(
        request: Request, project: str = Query(default="")
    ) -> JSONResponse:
        body = await request.json()
        name = body.get("name", "")
        location = body.get("location", "US")
        if not name:
            return JSONResponse(
                {"error": {"code": 400, "message": "Bucket name required"}},
                status_code=400,
            )
        bucket = await service.create_bucket(name, location=location)
        return JSONResponse(_bucket_json(bucket), status_code=200)

    @router.get("/storage/v1/b")
    async def list_buckets(project: str = Query(default="")) -> JSONResponse:
        buckets = await service.list_buckets()
        return JSONResponse(
            {
                "kind": "storage#buckets",
                "items": [_bucket_json(b) for b in buckets],
            }
        )

    @router.get("/storage/v1/b/{bucket}")
    async def get_bucket(bucket: str) -> JSONResponse:
        try:
            b = await service.get_bucket(bucket)
        except NotFoundError:
            return JSONResponse(
                {"error": {"code": 404, "message": f"Bucket {bucket!r} not found"}},
                status_code=404,
            )
        return JSONResponse(_bucket_json(b))

    @router.delete("/storage/v1/b/{bucket}")
    async def delete_bucket(bucket: str) -> Response:
        try:
            await service.delete_bucket(bucket)
        except NotFoundError:
            return JSONResponse(
                {"error": {"code": 404, "message": f"Bucket {bucket!r} not found"}},
                status_code=404,
            )
        return Response(status_code=204)

    # ------------------------------------------------------------------
    # Objects
    # ------------------------------------------------------------------

    @router.post("/upload/storage/v1/b/{bucket}/o")
    async def upload_object(
        bucket: str,
        request: Request,
        name: str = Query(default=""),
        uploadType: str = Query(default="media"),
    ) -> JSONResponse:
        if uploadType == "resumable":
            # Initiate resumable upload: respond with a Location header
            body_bytes = await request.body()
            metadata: dict = {}
            if body_bytes:
                try:
                    metadata = json.loads(body_bytes)
                except Exception:
                    pass
            object_name = unquote(metadata.get("name", name) or name)
            if not object_name:
                return JSONResponse(
                    {"error": {"code": 400, "message": "Object name required"}},
                    status_code=400,
                )
            content_type = metadata.get(
                "contentType",
                request.headers.get(
                    "x-upload-content-type", "application/octet-stream"
                ),
            )
            upload_id = uuid.uuid4().hex
            _pending_resumable[upload_id] = {
                "bucket": bucket,
                "name": object_name,
                "content_type": content_type,
            }
            base = str(request.base_url).rstrip("/")
            location = f"{base}/upload/storage/v1/b/{bucket}/o?uploadType=resumable&upload_id={upload_id}"
            return Response(status_code=200, headers={"Location": location})

        if uploadType == "media":
            data = await request.body()
            content_type = request.headers.get(
                "content-type", "application/octet-stream"
            )
            object_name = unquote(name)
        elif uploadType == "multipart":
            # Parse multipart/related body.
            # Part 1: JSON metadata (contains "name" and "contentType")
            # Part 2: Binary object data
            content_type_header = request.headers.get("content-type", "")
            boundary = ""
            for part in content_type_header.split(";"):
                p = part.strip()
                if p.startswith("boundary="):
                    boundary = p[len("boundary=") :].strip('"')
            if not boundary:
                return JSONResponse(
                    {"error": {"code": 400, "message": "Missing multipart boundary"}},
                    status_code=400,
                )

            raw = await request.body()
            # Split on boundaries; skip preamble
            delimiter = f"--{boundary}".encode()
            parts = raw.split(delimiter)
            # parts[0] = preamble (empty), parts[1..] = actual parts, last = '--' suffix
            metadata_raw = b""
            data = b""
            content_type = "application/octet-stream"
            for raw_part in parts[1:]:
                if raw_part.strip() in (b"--", b"--\r\n", b""):
                    continue
                # Strip leading \r\n
                raw_part = raw_part.lstrip(b"\r\n")
                # Split headers from body
                if b"\r\n\r\n" in raw_part:
                    headers_raw, body = raw_part.split(b"\r\n\r\n", 1)
                else:
                    continue
                # Strip trailing \r\n from body
                body = body.rstrip(b"\r\n")
                headers_str = headers_raw.decode("utf-8", errors="replace")
                part_ct = "application/octet-stream"
                for line in headers_str.splitlines():
                    if line.lower().startswith("content-type:"):
                        part_ct = line.split(":", 1)[1].strip()
                if "application/json" in part_ct:
                    metadata_raw = body
                else:
                    data = body
                    content_type = part_ct

            try:
                meta = json.loads(metadata_raw)
            except Exception:
                meta = {}
            object_name = unquote(meta.get("name", name))
            content_type = meta.get("contentType", content_type) or content_type
        else:
            return JSONResponse(
                {
                    "error": {
                        "code": 400,
                        "message": f"Unknown uploadType: {uploadType}",
                    }
                },
                status_code=400,
            )

        if not object_name:
            return JSONResponse(
                {"error": {"code": 400, "message": "Object name required"}},
                status_code=400,
            )

        try:
            obj = await service.upload_object(
                bucket, object_name, data, content_type=content_type
            )
        except NotFoundError:
            return JSONResponse(
                {"error": {"code": 404, "message": f"Bucket {bucket!r} not found"}},
                status_code=404,
            )
        return JSONResponse(
            _object_json(obj, bucket, str(request.base_url).rstrip("/")),
            status_code=200,
        )

    # ------------------------------------------------------------------
    # Resumable upload session (PUT to the session URL returned by POST)
    # ------------------------------------------------------------------

    @router.put("/upload/storage/v1/b/{bucket}/o")
    async def resumable_upload_put(
        bucket: str,
        request: Request,
        upload_id: str = Query(default=""),
    ) -> JSONResponse:
        pending = _pending_resumable.pop(upload_id, None)
        if not pending:
            return JSONResponse(
                {"error": {"code": 404, "message": "Upload session not found"}},
                status_code=404,
            )
        data = await request.body()
        content_type = request.headers.get(
            "content-type", pending.get("content_type", "application/octet-stream")
        )
        object_name = pending["name"]
        bucket_name = pending["bucket"]
        try:
            obj = await service.upload_object(
                bucket_name, object_name, data, content_type=content_type
            )
        except NotFoundError:
            return JSONResponse(
                {
                    "error": {
                        "code": 404,
                        "message": f"Bucket {bucket_name!r} not found",
                    }
                },
                status_code=404,
            )
        return JSONResponse(
            _object_json(obj, bucket_name, str(request.base_url).rstrip("/")),
            status_code=200,
        )

    # ------------------------------------------------------------------
    # Object read/delete
    # ------------------------------------------------------------------

    @router.get("/storage/v1/b/{bucket}/o")
    async def list_objects(
        bucket: str, request: Request, prefix: str = Query(default="")
    ) -> JSONResponse:
        try:
            objects = await service.list_objects(bucket, prefix=prefix)
        except NotFoundError:
            return JSONResponse(
                {"error": {"code": 404, "message": f"Bucket {bucket!r} not found"}},
                status_code=404,
            )
        base = str(request.base_url).rstrip("/")
        return JSONResponse(
            {
                "kind": "storage#objects",
                "items": [_object_json(o, bucket, base) for o in objects],
            }
        )

    @router.get("/storage/v1/b/{bucket}/o/{object_name:path}")
    async def get_or_download_object(
        bucket: str,
        object_name: str,
        request: Request,
        alt: str = Query(default="json"),
    ) -> Response:
        decoded_name = unquote(object_name)
        try:
            obj = await service.get_object(bucket, decoded_name)
        except NotFoundError:
            return JSONResponse(
                {
                    "error": {
                        "code": 404,
                        "message": f"{bucket}/{decoded_name} not found",
                    }
                },
                status_code=404,
            )
        if alt == "media":
            return Response(
                content=bytes(obj.data) if obj.data else b"",
                media_type=obj.content_type or "application/octet-stream",
            )
        return JSONResponse(
            _object_json(obj, bucket, str(request.base_url).rstrip("/"))
        )

    @router.delete("/storage/v1/b/{bucket}/o/{object_name:path}")
    async def delete_object(bucket: str, object_name: str) -> Response:
        decoded_name = unquote(object_name)
        try:
            await service.delete_object(bucket, decoded_name)
        except NotFoundError:
            return JSONResponse(
                {
                    "error": {
                        "code": 404,
                        "message": f"{bucket}/{decoded_name} not found",
                    }
                },
                status_code=404,
            )
        return Response(status_code=204)

    # The SDK also uses /download/... for alt=media, alias it:
    @router.get("/download/storage/v1/b/{bucket}/o/{object_name:path}")
    async def download_object(bucket: str, object_name: str) -> Response:
        decoded_name = unquote(object_name)
        try:
            obj = await service.get_object(bucket, decoded_name)
        except NotFoundError:
            return JSONResponse(
                {
                    "error": {
                        "code": 404,
                        "message": f"{bucket}/{decoded_name} not found",
                    }
                },
                status_code=404,
            )
        return Response(
            content=bytes(obj.data) if obj.data else b"",
            media_type=obj.content_type or "application/octet-stream",
        )

    return router
