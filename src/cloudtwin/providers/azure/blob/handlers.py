"""
Azure Blob Storage HTTP handlers.

Implements the Azure Blob Storage REST API subset that the azure-storage-blob
SDK uses when pointed at a custom endpoint (Azurite-compatible).

All routes are mounted under /{account_name}/ by register().
"""

from __future__ import annotations

import logging
from datetime import datetime, timezone
from xml.etree.ElementTree import Element, SubElement, tostring

from fastapi import APIRouter, Request, Response

from cloudtwin.core.errors import NotFoundError
from cloudtwin.providers.azure.blob.service import BlobService

log = logging.getLogger("cloudtwin.azure.blob")


def _to_rfc1123(iso: str) -> str:
    """Convert ISO 8601 timestamp to RFC 1123 HTTP date format required by Azure SDK."""
    try:
        dt = datetime.fromisoformat(iso)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.strftime("%a, %d %b %Y %H:%M:%S GMT")
    except (ValueError, TypeError):
        return iso


def _xml(root: Element) -> Response:
    body = (
        b'<?xml version="1.0" encoding="UTF-8"?>'
        + tostring(root, encoding="unicode").encode()
    )
    return Response(content=body, media_type="application/xml")


def _error_xml(code: str, message: str, status: int = 400) -> Response:
    root = Element("Error")
    SubElement(root, "Code").text = code
    SubElement(root, "Message").text = message
    body = (
        b'<?xml version="1.0" encoding="UTF-8"?>'
        + tostring(root, encoding="unicode").encode()
    )
    return Response(content=body, media_type="application/xml", status_code=status)


def make_router(service: BlobService) -> APIRouter:
    router = APIRouter()

    # ------------------------------------------------------------------
    # Account-level: list containers
    # ------------------------------------------------------------------

    @router.get("/")
    @router.get("")
    async def list_containers(request: Request) -> Response:
        comp = request.query_params.get("comp", "")
        if comp != "list":
            return Response(status_code=400, content=b"Unsupported comp parameter")
        prefix = request.query_params.get("prefix", "")
        containers = await service.list_containers()
        # ServiceEndpoint must be the full account URL (Azurite format)
        service_endpoint = str(request.base_url).rstrip("/") + "/"
        root = Element("EnumerationResults", ServiceEndpoint=service_endpoint)
        SubElement(root, "Prefix").text = prefix
        SubElement(root, "Marker")
        SubElement(root, "MaxResults").text = "5000"
        containers_el = SubElement(root, "Containers")
        for c in containers:
            if prefix and not c.name.startswith(prefix):
                continue
            ce = SubElement(containers_el, "Container")
            SubElement(ce, "Name").text = c.name
            props = SubElement(ce, "Properties")
            SubElement(props, "Last-Modified").text = _to_rfc1123(c.created_at)
            SubElement(props, "Etag").text = f'"{c.created_at}"'
            SubElement(props, "LeaseStatus").text = "unlocked"
            SubElement(props, "LeaseState").text = "available"
            SubElement(props, "DefaultEncryptionScope").text = "$account-encryption-key"
            SubElement(props, "DenyEncryptionScopeOverride").text = "false"
            SubElement(props, "ImmutableStorageWithVersioningEnabled").text = "false"
            SubElement(props, "HasImmutabilityPolicy").text = "false"
            SubElement(props, "HasLegalHold").text = "false"
        SubElement(root, "NextMarker")
        return _xml(root)

    # ------------------------------------------------------------------
    # Container operations  (path param + restype=container query)
    # ------------------------------------------------------------------

    @router.put("/{container}")
    async def put_container(container: str, request: Request) -> Response:
        restype = request.query_params.get("restype", "")
        if restype != "container":
            return Response(status_code=400)
        try:
            await service.create_container(container)
        except Exception:
            pass  # idempotent if already exists
        return Response(status_code=201)

    @router.delete("/{container}")
    async def delete_container(container: str, request: Request) -> Response:
        restype = request.query_params.get("restype", "")
        if restype != "container":
            return Response(status_code=400)
        try:
            await service.delete_container(container)
        except NotFoundError:
            return _error_xml(
                "ContainerNotFound", f"Container {container!r} does not exist.", 404
            )
        return Response(status_code=202)

    @router.get("/{container}")
    async def get_container_or_list_blobs(container: str, request: Request) -> Response:
        restype = request.query_params.get("restype", "")
        comp = request.query_params.get("comp", "")
        prefix = request.query_params.get("prefix", "")

        if restype == "container" and comp == "list":
            try:
                blobs = await service.list_blobs(container, prefix=prefix)
            except NotFoundError:
                return _error_xml(
                    "ContainerNotFound", f"Container {container!r} does not exist.", 404
                )
            root = Element(
                "EnumerationResults",
                ContainerName=container,
                ServiceEndpoint=str(request.base_url),
            )
            SubElement(root, "Prefix").text = prefix
            SubElement(root, "Marker")
            SubElement(root, "MaxResults").text = "5000"
            blobs_el = SubElement(root, "Blobs")
            for b in blobs:
                be = SubElement(blobs_el, "Blob")
                SubElement(be, "Name").text = b.name
                props = SubElement(be, "Properties")
                SubElement(props, "Last-Modified").text = _to_rfc1123(b.created_at)
                SubElement(props, "Etag").text = f'"{b.created_at}"'
                SubElement(props, "Content-Length").text = str(b.content_length or 0)
                SubElement(props, "Content-Type").text = (
                    b.content_type or "application/octet-stream"
                )
                SubElement(props, "BlobType").text = "BlockBlob"
                SubElement(props, "LeaseStatus").text = "unlocked"
                SubElement(props, "LeaseState").text = "available"
            SubElement(root, "NextMarker")
            return _xml(root)

        # GET container properties
        if restype == "container":
            try:
                c = await service.get_container(container)
            except NotFoundError:
                return _error_xml(
                    "ContainerNotFound", f"Container {container!r} does not exist.", 404
                )
            return Response(
                status_code=200,
                headers={
                    "ETag": f'"{c.created_at}"',
                    "Last-Modified": _to_rfc1123(c.created_at),
                },
            )
        return Response(status_code=400)

    @router.head("/{container}")
    async def head_container(container: str, request: Request) -> Response:
        restype = request.query_params.get("restype", "")
        if restype == "container":
            try:
                c = await service.get_container(container)
            except NotFoundError:
                return Response(status_code=404)
            return Response(
                status_code=200,
                headers={
                    "ETag": f'"{c.created_at}"',
                    "Last-Modified": _to_rfc1123(c.created_at),
                },
            )
        return Response(status_code=400)

    # ------------------------------------------------------------------
    # Blob operations
    # ------------------------------------------------------------------

    @router.put("/{container}/{blob_name:path}")
    async def put_blob(container: str, blob_name: str, request: Request) -> Response:
        data = await request.body()
        content_type = request.headers.get("content-type", "application/octet-stream")
        try:
            blob = await service.put_blob(
                container, blob_name, data, content_type=content_type
            )
        except NotFoundError as exc:
            return _error_xml("ContainerNotFound", exc.message, 404)
        return Response(
            status_code=201,
            headers={
                "ETag": f'"{blob.created_at}"',
                "Last-Modified": _to_rfc1123(blob.created_at),
            },
        )

    @router.get("/{container}/{blob_name:path}")
    async def get_blob(container: str, blob_name: str, request: Request) -> Response:
        try:
            blob = await service.get_blob(container, blob_name)
        except NotFoundError:
            return _error_xml(
                "BlobNotFound", f"{container}/{blob_name} does not exist.", 404
            )

        data = bytes(blob.data) if blob.data else b""
        total = len(data)

        # Parse Range header – Azure SDK always sends Range: bytes=X-Y
        range_header = request.headers.get("range", "")
        if range_header.startswith("bytes="):
            range_val = range_header[6:]
            parts = range_val.split("-", 1)
            start = int(parts[0]) if parts[0] else 0
            end = int(parts[1]) if parts[1] else max(total - 1, 0)
            end = min(end, total - 1)
            chunk = data[start : end + 1]
            return Response(
                content=chunk,
                status_code=206,
                media_type=blob.content_type or "application/octet-stream",
                headers={
                    "ETag": f'"{blob.created_at}"',
                    "Last-Modified": _to_rfc1123(blob.created_at),
                    "Content-Length": str(len(chunk)),
                    "Content-Range": f"bytes {start}-{end}/{total}",
                    "Accept-Ranges": "bytes",
                },
            )

        # Full download (no Range header)
        return Response(
            content=data,
            media_type=blob.content_type or "application/octet-stream",
            headers={
                "ETag": f'"{blob.created_at}"',
                "Last-Modified": _to_rfc1123(blob.created_at),
                "Content-Length": str(total),
                "Content-Range": f"bytes 0-{max(total - 1, 0)}/{total}",
                "Accept-Ranges": "bytes",
            },
        )

    @router.head("/{container}/{blob_name:path}")
    async def head_blob(container: str, blob_name: str) -> Response:
        try:
            blob = await service.get_blob(container, blob_name)
        except NotFoundError:
            return Response(status_code=404)
        total = len(blob.data) if blob.data else 0
        return Response(
            status_code=200,
            headers={
                "ETag": f'"{blob.created_at}"',
                "Last-Modified": _to_rfc1123(blob.created_at),
                "Content-Length": str(total),
                "Content-Type": blob.content_type or "application/octet-stream",
                "Accept-Ranges": "bytes",
            },
        )

    @router.delete("/{container}/{blob_name:path}")
    async def delete_blob(container: str, blob_name: str) -> Response:
        try:
            await service.delete_blob(container, blob_name)
        except NotFoundError:
            return _error_xml(
                "BlobNotFound", f"{container}/{blob_name} does not exist.", 404
            )
        return Response(status_code=202)

    return router
