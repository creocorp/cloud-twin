"""AWS DynamoDB — HTTP handlers (JSON protocol, json_router dispatch)."""

from __future__ import annotations

import json
import logging

from fastapi import Request, Response
from fastapi.responses import JSONResponse

from cloudtwin.core.errors import CloudTwinError, NotFoundError
from cloudtwin.providers.aws.dynamodb.service import DynamoDBService
from cloudtwin.providers.aws.protocols.json_protocol import JsonProtocolRouter

log = logging.getLogger("cloudtwin.aws.dynamodb")

_REGION = "us-east-1"
_ACCOUNT_ID = "000000000000"

_PREFIX = "DynamoDB_20120810"


def _error(code: str, message: str, status: int = 400) -> JSONResponse:
    return JSONResponse({"__type": code, "message": message}, status_code=status)


def register_dynamodb_handlers(
    router: JsonProtocolRouter, service: DynamoDBService
) -> None:
    """Register all DynamoDB JSON-protocol action handlers into the shared json_router."""

    async def create_table(request: Request, body: dict) -> Response:
        try:
            table = await service.create_table(
                name=body["TableName"],
                key_schema=body.get("KeySchema", []),
                attribute_definitions=body.get("AttributeDefinitions", []),
            )
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse(
            {
                "TableDescription": {
                    "TableName": table.name,
                    "TableArn": f"arn:aws:dynamodb:{_REGION}:{_ACCOUNT_ID}:table/{table.name}",
                    "TableStatus": "ACTIVE",
                    "CreationDateTime": table.created_at,
                }
            }
        )

    async def describe_table(request: Request, body: dict) -> Response:
        try:
            table = await service.describe_table(body["TableName"])
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 400)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse(
            {
                "Table": {
                    "TableName": table.name,
                    "TableArn": f"arn:aws:dynamodb:{_REGION}:{_ACCOUNT_ID}:table/{table.name}",
                    "TableStatus": "ACTIVE",
                    "KeySchema": json.loads(table.key_schema),
                    "AttributeDefinitions": json.loads(table.attribute_definitions),
                    "CreationDateTime": table.created_at,
                }
            }
        )

    async def list_tables(request: Request, body: dict) -> Response:
        try:
            names = await service.list_tables()
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"TableNames": names})

    async def delete_table(request: Request, body: dict) -> Response:
        try:
            await service.delete_table(body["TableName"])
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 400)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({})

    async def put_item(request: Request, body: dict) -> Response:
        try:
            await service.put_item(body["TableName"], body.get("Item", {}))
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 400)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({})

    async def get_item(request: Request, body: dict) -> Response:
        try:
            item = await service.get_item(body["TableName"], body.get("Key", {}))
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 400)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"Item": item} if item else {})

    async def delete_item(request: Request, body: dict) -> Response:
        try:
            await service.delete_item(body["TableName"], body.get("Key", {}))
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 400)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({})

    async def scan(request: Request, body: dict) -> Response:
        try:
            items = await service.scan(body["TableName"])
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 400)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"Items": items, "Count": len(items)})

    async def query(request: Request, body: dict) -> Response:
        try:
            raw_vals = body.get("ExpressionAttributeValues", {})
            # Keep DynamoDB typed format: {":pk": {"S": "val"}} → {"pk": {"S": "val"}}
            key_vals = {k.lstrip(":"): v for k, v in raw_vals.items()}
            items = await service.query(body["TableName"], key_vals)
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 400)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"Items": items, "Count": len(items)})

    async def update_item(request: Request, body: dict) -> Response:
        # Basic UpdateItem: treat as PutItem of the key with the update expression
        # For simplicity, read existing item + merge updates
        try:
            table_name = body["TableName"]
            key = body.get("Key", {})
            existing = await service.get_item(table_name, key)
            merged = {**(existing or {}), **key}
            # Apply ExpressionAttributeValues as simple attribute updates
            for attr_val in body.get("ExpressionAttributeValues", {}).values():
                if isinstance(attr_val, dict):
                    for attr_type, attr_v in attr_val.items():
                        pass  # basic stub; merge key fields only
            await service.put_item(table_name, merged)
        except NotFoundError as exc:
            return _error("ResourceNotFoundException", exc.message, 400)
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({})

    async def batch_write_item(request: Request, body: dict) -> Response:
        try:
            for table_name, ops in body.get("RequestItems", {}).items():
                for op in ops:
                    if "PutRequest" in op:
                        await service.put_item(table_name, op["PutRequest"]["Item"])
                    elif "DeleteRequest" in op:
                        await service.delete_item(
                            table_name, op["DeleteRequest"]["Key"]
                        )
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"UnprocessedItems": {}})

    async def batch_get_item(request: Request, body: dict) -> Response:
        responses: dict = {}
        try:
            for table_name, req in body.get("RequestItems", {}).items():
                table_responses = []
                for key_dict in req.get("Keys", []):
                    item = await service.get_item(table_name, key_dict)
                    if item:
                        table_responses.append(item)
                responses[table_name] = table_responses
        except CloudTwinError as exc:
            return _error(exc.code, exc.message, exc.http_status)
        return JSONResponse({"Responses": responses, "UnprocessedKeys": {}})

    router.register(f"{_PREFIX}.CreateTable", create_table)
    router.register(f"{_PREFIX}.DescribeTable", describe_table)
    router.register(f"{_PREFIX}.ListTables", list_tables)
    router.register(f"{_PREFIX}.DeleteTable", delete_table)
    router.register(f"{_PREFIX}.PutItem", put_item)
    router.register(f"{_PREFIX}.GetItem", get_item)
    router.register(f"{_PREFIX}.DeleteItem", delete_item)
    router.register(f"{_PREFIX}.Scan", scan)
    router.register(f"{_PREFIX}.Query", query)
    router.register(f"{_PREFIX}.UpdateItem", update_item)
    router.register(f"{_PREFIX}.BatchWriteItem", batch_write_item)
    router.register(f"{_PREFIX}.BatchGetItem", batch_get_item)
