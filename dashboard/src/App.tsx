import { HashRouter, Routes, Route, Navigate } from "react-router-dom";
import { Layout } from "./components/layout/Layout";
import { OverviewPage } from "./pages/Overview";
import { EventLogPage } from "./pages/EventLog";
import { SESPage } from "./pages/aws/SES";
import { S3Page } from "./pages/aws/S3";
import { SNSPage } from "./pages/aws/SNS";
import { SQSPage } from "./pages/aws/SQS";
import { AzureBlobPage } from "./pages/azure/Blob";
import { ServiceBusPage } from "./pages/azure/ServiceBus";
import { GCSPage } from "./pages/gcp/Storage";
import { PubSubPage } from "./pages/gcp/PubSub";

export default function App() {
  return (
    <HashRouter>
      <Routes>
        <Route element={<Layout />}>
          <Route index element={<OverviewPage />} />
          <Route path="events" element={<EventLogPage />} />

          {/* AWS */}
          <Route path="aws/ses" element={<SESPage />} />
          <Route path="aws/s3" element={<S3Page />} />
          <Route path="aws/sns" element={<SNSPage />} />
          <Route path="aws/sqs" element={<SQSPage />} />

          {/* Azure */}
          <Route path="azure/blob" element={<AzureBlobPage />} />
          <Route path="azure/servicebus" element={<ServiceBusPage />} />

          {/* GCP */}
          <Route path="gcp/storage" element={<GCSPage />} />
          <Route path="gcp/pubsub" element={<PubSubPage />} />

          {/* Fallback */}
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
