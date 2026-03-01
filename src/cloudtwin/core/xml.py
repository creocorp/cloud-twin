"""
XML helpers for AWS-compatible responses.
"""

from __future__ import annotations

from xml.etree.ElementTree import Element, SubElement, tostring

AWS_NS = "https://email.amazonaws.com/doc/2010-12-01/"
S3_NS = "http://s3.amazonaws.com/doc/2006-03-01/"
SNS_NS = "http://sns.amazonaws.com/doc/2010-03-31/"


def _to_xml_bytes(root: Element) -> bytes:
    return (
        b'<?xml version="1.0" encoding="UTF-8"?>\n'
        + tostring(root, encoding="unicode").encode()
    )


# ---------------------------------------------------------------------------
# SES helpers
# ---------------------------------------------------------------------------


def ses_response(action: str, inner_fn) -> bytes:
    """
    Wrap an inner builder in a standard SES response envelope.

    Example output:
        <VerifyDomainIdentityResponse xmlns="...">
          <VerifyDomainIdentityResult>...</VerifyDomainIdentityResult>
          <ResponseMetadata><RequestId>...</RequestId></ResponseMetadata>
        </VerifyDomainIdentityResponse>
    """
    import uuid

    root = Element(f"{action}Response", xmlns=AWS_NS)
    result = SubElement(root, f"{action}Result")
    inner_fn(result)
    meta = SubElement(root, "ResponseMetadata")
    SubElement(meta, "RequestId").text = str(uuid.uuid4())
    return _to_xml_bytes(root)


# ---------------------------------------------------------------------------
# SNS helpers
# ---------------------------------------------------------------------------


def sns_response(action: str, inner_fn) -> bytes:
    """
    Wrap an inner builder in a standard SNS response envelope.

    Example output:
        <CreateTopicResponse xmlns="...">
          <CreateTopicResult>...</CreateTopicResult>
          <ResponseMetadata><RequestId>...</RequestId></ResponseMetadata>
        </CreateTopicResponse>
    """
    import uuid

    root = Element(f"{action}Response", xmlns=SNS_NS)
    result = SubElement(root, f"{action}Result")
    inner_fn(result)
    meta = SubElement(root, "ResponseMetadata")
    SubElement(meta, "RequestId").text = str(uuid.uuid4())
    return _to_xml_bytes(root)


def sns_error_response(code: str, message: str) -> bytes:
    import uuid

    root = Element("ErrorResponse", xmlns=SNS_NS)
    error = SubElement(root, "Error")
    SubElement(error, "Type").text = "Sender"
    SubElement(error, "Code").text = code
    SubElement(error, "Message").text = message
    meta = SubElement(root, "RequestId")
    meta.text = str(uuid.uuid4())
    return _to_xml_bytes(root)


def ses_error_response(code: str, message: str, http_status: int = 400) -> bytes:
    import uuid

    root = Element("ErrorResponse", xmlns=AWS_NS)
    error = SubElement(root, "Error")
    SubElement(error, "Type").text = "Sender"
    SubElement(error, "Code").text = code
    SubElement(error, "Message").text = message
    meta = SubElement(root, "RequestId")
    meta.text = str(uuid.uuid4())
    return _to_xml_bytes(root)
