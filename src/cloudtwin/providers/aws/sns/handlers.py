"""
SNS HTTP handlers (AWS Query / XML protocol).

All SNS operations arrive as POST / with Content-Type
application/x-www-form-urlencoded and an 'Action' field.
"""

from __future__ import annotations

from fastapi import Request, Response

from cloudtwin.core.errors import CloudTwinError
from cloudtwin.core.xml import sns_error_response, sns_response
from cloudtwin.providers.aws.protocols.query import QueryProtocolRouter
from cloudtwin.providers.aws.sns.service import SnsService


def register_sns_handlers(router: QueryProtocolRouter, service: SnsService) -> None:
    """Register all SNS Query-protocol action handlers into the shared router."""

    async def create_topic(request: Request, params: dict) -> Response:
        name = params.get("Name", "")
        if not name:
            return Response(
                content=sns_error_response("InvalidParameter", "Name is required"),
                status_code=400,
                media_type="text/xml",
            )
        try:
            arn = await service.create_topic(name)
        except CloudTwinError as exc:
            return Response(
                content=sns_error_response(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="text/xml",
            )

        def build(result):
            from xml.etree.ElementTree import SubElement

            SubElement(result, "TopicArn").text = arn

        return Response(
            content=sns_response("CreateTopic", build), media_type="text/xml"
        )

    async def delete_topic(request: Request, params: dict) -> Response:
        topic_arn = params.get("TopicArn", "")
        if not topic_arn:
            return Response(
                content=sns_error_response("InvalidParameter", "TopicArn is required"),
                status_code=400,
                media_type="text/xml",
            )
        try:
            await service.delete_topic(topic_arn)
        except CloudTwinError as exc:
            return Response(
                content=sns_error_response(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="text/xml",
            )
        return Response(
            content=sns_response("DeleteTopic", lambda r: None), media_type="text/xml"
        )

    async def list_topics(request: Request, params: dict) -> Response:
        try:
            arns = await service.list_topics()
        except CloudTwinError as exc:
            return Response(
                content=sns_error_response(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="text/xml",
            )

        def build(result):
            from xml.etree.ElementTree import SubElement

            topics_el = SubElement(result, "Topics")
            for arn in arns:
                member = SubElement(topics_el, "member")
                SubElement(member, "TopicArn").text = arn
            SubElement(result, "NextToken")

        return Response(
            content=sns_response("ListTopics", build), media_type="text/xml"
        )

    async def subscribe(request: Request, params: dict) -> Response:
        topic_arn = params.get("TopicArn", "")
        protocol = params.get("Protocol", "")
        endpoint = params.get("Endpoint", "")
        if not topic_arn or not protocol:
            return Response(
                content=sns_error_response(
                    "InvalidParameter", "TopicArn and Protocol are required"
                ),
                status_code=400,
                media_type="text/xml",
            )
        try:
            sub_arn = await service.subscribe(topic_arn, protocol, endpoint)
        except CloudTwinError as exc:
            return Response(
                content=sns_error_response(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="text/xml",
            )

        def build(result):
            from xml.etree.ElementTree import SubElement

            SubElement(result, "SubscriptionArn").text = sub_arn

        return Response(content=sns_response("Subscribe", build), media_type="text/xml")

    async def unsubscribe(request: Request, params: dict) -> Response:
        subscription_arn = params.get("SubscriptionArn", "")
        if not subscription_arn:
            return Response(
                content=sns_error_response(
                    "InvalidParameter", "SubscriptionArn is required"
                ),
                status_code=400,
                media_type="text/xml",
            )
        try:
            await service.unsubscribe(subscription_arn)
        except CloudTwinError as exc:
            return Response(
                content=sns_error_response(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="text/xml",
            )
        return Response(
            content=sns_response("Unsubscribe", lambda r: None), media_type="text/xml"
        )

    async def list_subscriptions(request: Request, params: dict) -> Response:
        try:
            arns = await service.list_subscriptions()
        except CloudTwinError as exc:
            return Response(
                content=sns_error_response(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="text/xml",
            )

        def build(result):
            from xml.etree.ElementTree import SubElement

            subs_el = SubElement(result, "Subscriptions")
            for arn in arns:
                member = SubElement(subs_el, "member")
                SubElement(member, "SubscriptionArn").text = arn
            SubElement(result, "NextToken")

        return Response(
            content=sns_response("ListSubscriptions", build), media_type="text/xml"
        )

    async def publish(request: Request, params: dict) -> Response:
        topic_arn = params.get("TopicArn", "")
        message = params.get("Message", "")
        subject = params.get("Subject") or None
        if not topic_arn or not message:
            return Response(
                content=sns_error_response(
                    "InvalidParameter", "TopicArn and Message are required"
                ),
                status_code=400,
                media_type="text/xml",
            )
        try:
            message_id = await service.publish(topic_arn, message, subject)
        except CloudTwinError as exc:
            return Response(
                content=sns_error_response(exc.code, exc.message),
                status_code=exc.http_status,
                media_type="text/xml",
            )

        def build(result):
            from xml.etree.ElementTree import SubElement

            SubElement(result, "MessageId").text = message_id

        return Response(content=sns_response("Publish", build), media_type="text/xml")

    router.register("CreateTopic", create_topic)
    router.register("DeleteTopic", delete_topic)
    router.register("ListTopics", list_topics)
    router.register("Subscribe", subscribe)
    router.register("Unsubscribe", unsubscribe)
    router.register("ListSubscriptions", list_subscriptions)
    router.register("Publish", publish)
