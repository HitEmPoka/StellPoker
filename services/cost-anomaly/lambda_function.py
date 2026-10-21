#!/usr/bin/env python3
"""
Cost Anomaly Detection Lambda Function

Detects abnormal daily spend spikes in StellPoker infrastructure.
Compares current day spend against 7-day and 30-day baselines.
"""

import json
import os
import logging
from datetime import datetime, timedelta
from typing import Dict, List, Tuple

import boto3

logger = logging.getLogger()
logger.setLevel(logging.INFO)

ce_client = boto3.client("ce")
sns_client = boto3.client("sns")
cloudwatch = boto3.client("cloudwatch")


def get_cost_data(start_date: str, end_date: str) -> Dict[str, float]:
    """
    Fetch cost data from AWS Cost Explorer for the given date range.
    Returns daily costs as {date: cost}.
    """
    try:
        response = ce_client.get_cost_and_usage(
            TimePeriod={"Start": start_date, "End": end_date},
            Granularity="DAILY",
            Metrics=["UnblendedCost"],
            GroupBy=[
                {"Type": "DIMENSION", "Key": "SERVICE"}
            ]
        )

        daily_costs = {}
        for result in response.get("ResultsByTime", []):
            date = result["TimePeriod"]["Start"]
            total_cost = sum(
                float(group["Metrics"]["UnblendedCost"]["Amount"])
                for group in result.get("Groups", [])
            )
            daily_costs[date] = total_cost

        return daily_costs

    except Exception as e:
        logger.error(f"Error fetching cost data: {e}")
        raise


def calculate_baseline(costs: List[float], window_days: int = 7) -> Tuple[float, float]:
    """
    Calculate baseline (mean) and standard deviation from historical costs.
    """
    if not costs or len(costs) < 2:
        return 0.0, 0.0

    mean = sum(costs) / len(costs)
    variance = sum((x - mean) ** 2 for x in costs) / len(costs)
    std_dev = variance ** 0.5

    return mean, std_dev


def detect_anomaly(current_cost: float, baseline: float, threshold: float) -> bool:
    """
    Detect if current cost is an anomaly based on threshold percentage.
    threshold: percentage increase (e.g., 25 for 25% increase)
    """
    if baseline == 0:
        return current_cost > 0

    percentage_change = ((current_cost - baseline) / baseline) * 100
    is_anomaly = percentage_change >= threshold

    logger.info(
        f"Current: ${current_cost:.2f}, Baseline: ${baseline:.2f}, "
        f"Change: {percentage_change:.1f}%, Threshold: {threshold}%, Anomaly: {is_anomaly}"
    )

    return is_anomaly


def publish_alert(
    service: str,
    current_cost: float,
    baseline_7d: float,
    baseline_30d: float,
    anomaly_type: str,
    threshold: float
) -> None:
    """Publish SNS alert for detected anomaly."""
    topic_arn = os.environ.get("SNS_TOPIC_ARN")
    if not topic_arn:
        logger.warning("SNS_TOPIC_ARN not configured, skipping alert")
        return

    percentage_change_7d = ((current_cost - baseline_7d) / baseline_7d * 100) if baseline_7d > 0 else 0
    percentage_change_30d = ((current_cost - baseline_30d) / baseline_30d * 100) if baseline_30d > 0 else 0

    message = f"""
StellPoker Cost Anomaly Alert
=============================

Service: {service}
Anomaly Type: {anomaly_type}
Threshold: {threshold}%

Current Daily Cost: ${current_cost:.2f}

7-Day Baseline:
  - Average: ${baseline_7d:.2f}
  - Change: {percentage_change_7d:+.1f}%

30-Day Baseline:
  - Average: ${baseline_30d:.2f}
  - Change: {percentage_change_30d:+.1f}%

Action Required:
1. Review recent infrastructure changes
2. Check for unexpected scaling or resource usage
3. Verify no rogue processes or security incidents
4. Consider scaling policies and cost optimization

Timestamp: {datetime.utcnow().isoformat()}Z
Environment: {os.environ.get("ENVIRONMENT", "production")}
"""

    try:
        sns_client.publish(
            TopicArn=topic_arn,
            Subject=f"[ALERT] Cost Anomaly Detected - {service}",
            Message=message
        )
        logger.info(f"Alert published for {service}")
    except Exception as e:
        logger.error(f"Failed to publish alert: {e}")


def put_metric(metric_name: str, value: float, unit: str = "None") -> None:
    """Publish custom CloudWatch metric."""
    try:
        cloudwatch.put_metric_data(
            Namespace="StellPoker/Cost",
            MetricData=[
                {
                    "MetricName": metric_name,
                    "Value": value,
                    "Unit": unit,
                    "Timestamp": datetime.utcnow()
                }
            ]
        )
    except Exception as e:
        logger.error(f"Failed to put metric {metric_name}: {e}")


def lambda_handler(event, context):
    """
    Main Lambda handler for cost anomaly detection.
    """
    threshold = float(os.environ.get("COST_ANOMALY_THRESHOLD", 25))
    log_group = os.environ.get("CLOUDWATCH_LOG_GROUP", "/stellpoker/cost-anomaly")

    logger.info(f"Starting cost anomaly detection with threshold: {threshold}%")

    try:
        today = datetime.utcnow().date()
        yesterday = today - timedelta(days=1)
        week_ago = today - timedelta(days=7)
        month_ago = today - timedelta(days=30)

        # Fetch cost data
        yesterday_str = yesterday.isoformat()
        today_str = today.isoformat()

        # Get yesterday's costs (since today may be incomplete)
        yesterday_costs = get_cost_data(yesterday_str, today_str)
        current_cost = yesterday_costs.get(yesterday_str, 0.0)

        # Get baseline periods
        baseline_7d_costs = get_cost_data(week_ago.isoformat(), yesterday_str)
        baseline_30d_costs = get_cost_data(month_ago.isoformat(), yesterday_str)

        # Remove yesterday from baselines
        if yesterday_str in baseline_7d_costs:
            del baseline_7d_costs[yesterday_str]
        if yesterday_str in baseline_30d_costs:
            del baseline_30d_costs[yesterday_str]

        baseline_7d, std_7d = calculate_baseline(list(baseline_7d_costs.values()), window_days=7)
        baseline_30d, std_30d = calculate_baseline(list(baseline_30d_costs.values()), window_days=30)

        # Publish metrics
        put_metric("DailySpend", current_cost, "None")
        put_metric("Baseline7Day", baseline_7d, "None")
        put_metric("Baseline30Day", baseline_30d, "None")

        # Detect anomalies
        anomaly_7d = detect_anomaly(current_cost, baseline_7d, threshold)
        anomaly_30d = detect_anomaly(current_cost, baseline_30d, threshold)

        if anomaly_7d or anomaly_30d:
            put_metric("AnomalyDetected", 1, "Count")
            anomaly_type = "7-Day" if anomaly_7d else "30-Day"
            publish_alert(
                service="StellPoker Infrastructure",
                current_cost=current_cost,
                baseline_7d=baseline_7d,
                baseline_30d=baseline_30d,
                anomaly_type=anomaly_type,
                threshold=threshold
            )
            logger.warning(f"ANOMALY DETECTED: {anomaly_type} - Current: ${current_cost:.2f}")
        else:
            put_metric("AnomalyDetected", 0, "Count")
            logger.info(f"No anomaly detected. Cost: ${current_cost:.2f}")

        return {
            "statusCode": 200,
            "body": json.dumps({
                "message": "Cost anomaly detection completed",
                "current_cost": current_cost,
                "baseline_7d": baseline_7d,
                "baseline_30d": baseline_30d,
                "anomaly_detected": anomaly_7d or anomaly_30d
            })
        }

    except Exception as e:
        logger.error(f"Unhandled error in cost anomaly detection: {e}", exc_info=True)
        return {
            "statusCode": 500,
            "body": json.dumps({"error": str(e)})
        }
