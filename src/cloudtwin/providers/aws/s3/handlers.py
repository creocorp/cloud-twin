"""
S3 REST-compatible HTTP handlers.

Mounted at /{bucket} and /{bucket}/{key:path} on the FastAPI app.
Supports path-style addressing only (MVP).
"""

from __future__ import annotations

from xml.etree.ElementTree import Element, SubElement, tostring

from fastapi import APIRouter, Request, Response
from fastapi.responses import Response as FR
from fastapi.responses import StreamingResponse

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.aws.s3.service import S3Service

S3_NS = "http://s3.amazonaws.com/doc/2006-03-01/"


def _xml(root: Element) -> bytes:
    return (
        b'<?xml version="1.0" encoding="UTF-8"?>\n'
        + tostring(root, encoding="unicode").encode()
    )


def _error_xml(code: str, message: str) -> bytes:
    root = Element("Error")
    SubElement(root, "Code").text = code
    SubElement(root, "Message").text = message
    return _xml(root)


def make_s3_router(service: S3Service) -> APIRouter:
    router = APIRouter()

    # ------------------------------------------------------------------
    # ListBuckets  GET /
    # ------------------------------------------------------------------
    @router.get("/")
    async def list_buckets(request: Request):
        buckets = await service.list_buckets()
        root = Element("ListAllMyBucketsResult", xmlns=S3_NS)
        owner = SubElement(root, "Owner")
        SubElement(owner, "ID").text = "cloudtwin"
        SubElement(owner, "DisplayName").text = "cloudtwin"
        blist = SubElement(root, "Buckets")
        for b in buckets:
            bel = SubElement(blist, "Bucket")
            SubElement(bel, "Name").text = b.name
            SubElement(bel, "CreationDate").text = b.created_at
        return FR(content=_xml(root), media_type="application/xml")

    # ------------------------------------------------------------------
    # CreateBucket  PUT /{bucket}
    # ------------------------------------------------------------------
    @router.put("/{bucket}")
    async def create_bucket(bucket: str, request: Request):
        try:
            await service.create_bucket(bucket)
        except CloudTwinError as exc:
            return FR(
                content=_error_xml(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="application/xml",
            )
        return FR(content=b"", status_code=200, headers={"Location": f"/{bucket}"})

    # ------------------------------------------------------------------
    # ListObjectsV2  GET /{bucket}?list-type=2
    # ListObjectsV2  GET /{bucket}
    # ------------------------------------------------------------------
    @router.get("/{bucket}")
    async def list_objects(bucket: str, request: Request):
        prefix = request.query_params.get("prefix", "")
        max_keys = int(request.query_params.get("max-keys", 1000))
        try:
            objects = await service.list_objects_v2(
                bucket, prefix=prefix, max_keys=max_keys
            )
        except NotFoundError as exc:
            return FR(
                content=_error_xml("NoSuchBucket", str(exc)),
                status_code=404,
                media_type="application/xml",
            )

        root = Element("ListBucketResult", xmlns=S3_NS)
        SubElement(root, "Name").text = bucket
        SubElement(root, "Prefix").text = prefix
        SubElement(root, "MaxKeys").text = str(max_keys)
        SubElement(root, "IsTruncated").text = "false"
        for obj in objects:
            content = SubElement(root, "Contents")
            SubElement(content, "Key").text = obj.key
            SubElement(content, "LastModified").text = obj.created_at
            SubElement(content, "Size").text = str(obj.content_length or 0)
            SubElement(content, "StorageClass").text = "STANDARD"
        return FR(content=_xml(root), media_type="application/xml")

    # ------------------------------------------------------------------
    # PutObject  PUT /{bucket}/{key}
    # ------------------------------------------------------------------
    @router.put("/{bucket}/{key:path}")
    async def put_object(bucket: str, key: str, request: Request):
        data = await request.body()
        content_type = request.headers.get("content-type", "application/octet-stream")
        try:
            obj = await service.put_object(bucket, key, data, content_type)
        except CloudTwinError as exc:
            return FR(
                content=_error_xml(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="application/xml",
            )
        import hashlib

        etag = hashlib.md5(data).hexdigest()
        return FR(content=b"", status_code=200, headers={"ETag": f'"{etag}"'})

    # ------------------------------------------------------------------
    # GetObject  GET /{bucket}/{key}
    # ------------------------------------------------------------------
    @router.get("/{bucket}/{key:path}")
    async def get_object(bucket: str, key: str, request: Request):
        try:
            obj = await service.get_object(bucket, key)
        except NotFoundError as exc:
            return FR(
                content=_error_xml("NoSuchKey", str(exc)),
                status_code=404,
                media_type="application/xml",
            )
        headers = {
            "Content-Type": obj.content_type or "application/octet-stream",
            "Content-Length": str(obj.content_length or 0),
            "Last-Modified": obj.created_at,
        }
        return FR(content=obj.data or b"", headers=headers)

    # ------------------------------------------------------------------
    # DeleteObject  DELETE /{bucket}/{key}
    # ------------------------------------------------------------------
    @router.delete("/{bucket}/{key:path}")
    async def delete_object(bucket: str, key: str, request: Request):
        try:
            await service.delete_object(bucket, key)
        except CloudTwinError as exc:
            return FR(
                content=_error_xml(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="application/xml",
            )
        return FR(content=b"", status_code=204)

    # ------------------------------------------------------------------
    # DeleteBucket  DELETE /{bucket}
    # ------------------------------------------------------------------
    @router.delete("/{bucket}")
    async def delete_bucket(bucket: str, request: Request):
        # Simple delegation – bucket repo delete
        try:
            bucket_obj = await service._bucket_repo.get(bucket)
            if bucket_obj is None:
                return FR(
                    content=_error_xml("NoSuchBucket", f"Bucket not found: {bucket}"),
                    status_code=404,
                    media_type="application/xml",
                )
            await service._bucket_repo.delete(bucket)
        except CloudTwinError as exc:
            return FR(
                content=_error_xml(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="application/xml",
            )
        return FR(content=b"", status_code=204)

    return router
