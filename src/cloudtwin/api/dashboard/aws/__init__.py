from cloudtwin.api.dashboard.aws import (
    bedrock,
    dynamodb,
    lambda_,
    s3,
    secretsmanager,
    ses,
    sns,
    sqs,
)

routers = [
    ses.router,
    s3.router,
    sns.router,
    sqs.router,
    lambda_.router,
    dynamodb.router,
    secretsmanager.router,
    bedrock.router,
]
