# Cost Anomaly Detection

## Overview

StellPoker implements automated cost anomaly detection to identify abnormal daily spending spikes in cloud infrastructure. This system compares current day costs against 7-day and 30-day historical baselines and triggers alerts when anomalies are detected.

## How It Works

1. **Daily Cost Fetch** (9 AM UTC): Lambda function queries AWS Cost Explorer
2. **Baseline Calculation**: Compares yesterday's cost against:
   - 7-day rolling average
   - 30-day rolling average
3. **Anomaly Detection**: Flagged if cost exceeds baseline by configured threshold (default: 25%)
4. **Alert Routing**: SNS topic publishes alert to configured email recipients

## Architecture

### Components

- **Lambda Function**: `stellpoker-cost-anomaly-{env}` (Python 3.11)
- **EventBridge Rule**: Daily trigger at 9 AM UTC
- **SNS Topic**: `stellpoker-cost-anomaly-{env}` for alert delivery
- **CloudWatch Dashboard**: Visual cost tracking and anomaly metrics

### Metrics Published

- `StellPoker/Cost/DailySpend` — Yesterday's total spend
- `StellPoker/Cost/Baseline7Day` — 7-day rolling average
- `StellPoker/Cost/Baseline30Day` — 30-day rolling average
- `StellPoker/Cost/AnomalyDetected` — Boolean (1 = anomaly, 0 = normal)

### Logs

CloudWatch Log Group: `/stellpoker/cost-anomaly/{environment}`
- Retention: 30 days
- Format: JSON with timestamp, cost values, baseline comparisons

## Deployment

### Terraform Variables

```hcl
variable "cost_anomaly_threshold" {
  default = 25  # Percentage increase to trigger alert
}

variable "cost_anomaly_email" {
  # Email for alert subscriptions (confirm SNS subscription when deployed)
}

variable "environment" {
  default = "production"
}
```

### Deploy

```bash
cd infrastructure/terraform
terraform init
terraform apply -var="cost_anomaly_email=alerts@stellpoker.com"
```

### Confirm Subscription

AWS SNS will send a confirmation email. Click the link to activate alerts.

## Interpreting Alerts

### Alert Example

```
StellPoker Cost Anomaly Alert
=============================

Service: StellPoker Infrastructure
Anomaly Type: 7-Day
Threshold: 25%

Current Daily Cost: $1,250.00

7-Day Baseline:
  - Average: $975.00
  - Change: +28.2%

Action Required:
1. Review recent infrastructure changes
2. Check for unexpected scaling or resource usage
3. Verify no rogue processes or security incidents
```

### Root Cause Analysis

When an alert fires:

1. **Review Recent Deployments**
   - Check `.github/workflows/*` for recent changes
   - Review `CHANGELOG.md` for new features

2. **Inspect AWS Console**
   - Cost Explorer: Which services are driving the increase?
   - EC2/ECS: Are new instances/tasks running unexpectedly?
   - Lambda: Check invocation rates and durations

3. **Check Logs**
   ```bash
   # View anomaly detection logs
   aws logs tail /stellpoker/cost-anomaly/production --follow
   ```

4. **Query Cost Details**
   ```bash
   # AWS CLI - get cost breakdown
   aws ce get-cost-and-usage \
     --time-period Start=2024-01-01,End=2024-01-02 \
     --granularity DAILY \
     --metrics "UnblendedCost" \
     --group-by Type=DIMENSION,Key=SERVICE
   ```

## Tuning

### Adjust Threshold

```bash
cd infrastructure/terraform
terraform apply -var="cost_anomaly_threshold=30"  # Increase to 30%
```

### Add More Recipients

```bash
# Add email subscription
aws sns subscribe \
  --topic-arn arn:aws:sns:us-east-1:ACCOUNT:stellpoker-cost-anomaly-production \
  --protocol email \
  --notification-endpoint another-alert@stellpoker.com
```

### Change Check Time

Edit `.github/workflows/release.yml` schedule expression:
```yaml
schedule_expression = "cron(0 9 * * ? *)"  # 9 AM UTC
```

## FAQ

**Q: False positive - why did I get an alert?**
A: Legitimate spikes occur due to:
- New features with higher compute
- Load tests or benchmarking
- One-time data migrations
- Seasonal traffic patterns

Review the alert details and update baseline if needed.

**Q: Can I suppress alerts?**
A: Temporarily unsubscribe from SNS topic, or increase threshold. Note: alerts are important for detecting security incidents or runaway costs.

**Q: How far back does the baseline go?**
A: 7-day and 30-day windows. We use 7-day for recent trends, 30-day for seasonal context.

**Q: Is this HIPAA/SOC2 compliant?**
A: Cost data is aggregated and anonymized. No PII is included. Alerts are encrypted in SNS (KMS).

## Related Issues

- **#351**: Cost monitoring baseline
- **#669**: Cost anomaly detection (this feature)

## See Also

- [AWS Cost Explorer Documentation](https://docs.aws.amazon.com/cost-management/latest/userguide/ce-what-is.html)
- [CloudWatch Dashboards](https://docs.aws.amazon.com/AmazonCloudWatch/latest/monitoring/CloudWatch_Dashboards.html)
- Infrastructure configuration: `infrastructure/terraform/cost_anomaly.tf`
