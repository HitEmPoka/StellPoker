# StellPoker Infrastructure Deployment Guide

This document provides comprehensive instructions for deploying StellPoker using Terraform on AWS and GCP.

For production Soroban data-plane operations, including self-hosted Stellar Core, Horizon, and Soroban RPC guidance, see [../docs/soroban-rpc-node.md](../docs/soroban-rpc-node.md).

## Prerequisites

### Required Tools

- **Terraform** >= 1.0: [Install Terraform](https://learn.hashicorp.com/tutorials/terraform/install-cli)
- **AWS CLI** >= 2.0: [Install AWS CLI](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html)
- **Google Cloud SDK**: [Install gcloud](https://cloud.google.com/sdk/docs/install)
- **kubectl**: [Install kubectl](https://kubernetes.io/docs/tasks/tools/)
- **Docker**: [Install Docker](https://docs.docker.com/get-docker/)

### AWS Setup

1. **Create AWS Account** and configure credentials:
   ```bash
   aws configure
   # Enter: Access Key ID, Secret Access Key, Region, Output format
   ```

2. **Create S3 bucket** for Terraform state (optional but recommended):
   ```bash
   aws s3api create-bucket \
     --bucket stellpoker-terraform-state-$(aws sts get-caller-identity --query Account --output text) \
     --region us-east-1
   ```

3. **Create DynamoDB table** for state locking:
   ```bash
   aws dynamodb create-table \
     --table-name terraform-locks \
     --attribute-definitions AttributeName=LockID,AttributeType=S \
     --key-schema AttributeName=LockID,KeyType=HASH \
     --billing-mode PAY_PER_REQUEST
   ```

### GCP Setup

1. **Create GCP Project**:
   ```bash
   gcloud projects create stellpoker-prod --name="StellPoker"
   ```

2. **Set project ID**:
   ```bash
   export GCP_PROJECT_ID=$(gcloud config get-value project)
   gcloud config set project $GCP_PROJECT_ID
   ```

3. **Enable required APIs**:
   ```bash
   gcloud services enable \
     container.googleapis.com \
     sqladmin.googleapis.com \
     compute.googleapis.com \
     servicenetworking.googleapis.com \
     cloudresourcemanager.googleapis.com \
     logging.googleapis.com
   ```

4. **Create service account**:
   ```bash
   gcloud iam service-accounts create terraform \
     --display-name="Terraform Service Account"
   
   gcloud projects add-iam-policy-binding $GCP_PROJECT_ID \
     --member="serviceAccount:terraform@$GCP_PROJECT_ID.iam.gserviceaccount.com" \
     --role="roles/editor"
   ```

5. **Create and download service account key**:
   ```bash
   gcloud iam service-accounts keys create /tmp/terraform-key.json \
     --iam-account=terraform@$GCP_PROJECT_ID.iam.gserviceaccount.com
   
   export GOOGLE_APPLICATION_CREDENTIALS=/tmp/terraform-key.json
   ```

## Deployment

### AWS Deployment

1. **Prepare Terraform variables**:
   ```bash
   cd infrastructure/terraform
   
   cat > terraform.tfvars << EOF
   environment              = "prod"
   aws_region              = "us-east-1"
   vpc_cidr                = "10.0.0.0/16"
   coordinator_container_image = "YOUR_ECR_REPO/coordinator:latest"
   mpc_node_container_image    = "YOUR_ECR_REPO/mpc-node:latest"
   db_password             = "YOUR_SECURE_PASSWORD"
   enable_monitoring       = true
   enable_cdn              = true
   alarm_email             = "ops@example.com"
   EOF
   ```

2. **Initialize Terraform**:
   ```bash
   terraform init \
     -backend-config="bucket=stellpoker-terraform-state-ACCOUNT_ID" \
     -backend-config="key=prod/terraform.tfstate" \
     -backend-config="region=us-east-1" \
     -backend-config="dynamodb_table=terraform-locks"
   ```

3. **Plan deployment**:
   ```bash
   terraform plan -out=tfplan
   ```

4. **Review and apply**:
   ```bash
   # Review the plan carefully
   terraform apply tfplan
   ```

5. **Get outputs**:
   ```bash
   terraform output -json > deployment-outputs.json
   
   # Get specific outputs
   terraform output alb_dns_name
   terraform output rds_endpoint
   terraform output cdn_domain_name
   ```

### GCP Deployment

1. **Prepare Terraform variables**:
   ```bash
   cat > terraform-gcp.tfvars << EOF
   gcp_project_id              = "$GCP_PROJECT_ID"
   gcp_region                  = "us-central1"
   environment                 = "prod"
   coordinator_container_image = "gcr.io/$GCP_PROJECT_ID/coordinator:latest"
   mpc_node_container_image    = "gcr.io/$GCP_PROJECT_ID/mpc-node:latest"
   db_password                 = "YOUR_SECURE_PASSWORD"
   enable_monitoring           = true
   enable_cdn                  = true
   gke_node_count              = 3
   gke_machine_type            = "e2-standard-4"
   EOF
   ```

2. **Initialize and deploy**:
   ```bash
   terraform init -upgrade
   terraform apply -var-file=terraform-gcp.tfvars
   ```

3. **Configure kubectl access**:
   ```bash
   gcloud container clusters get-credentials stellpoker-gke-cluster \
     --region us-central1 \
     --project $GCP_PROJECT_ID
   
   kubectl get nodes
   ```

## Post-Deployment Configuration

### 1. Build and Push Docker Images

#### AWS (ECR):
```bash
aws ecr create-repository --repository-name coordinator --region us-east-1
aws ecr create-repository --repository-name mpc-node --region us-east-1

# Get ECR login token
aws ecr get-login-password --region us-east-1 | \
  docker login --username AWS --password-stdin ACCOUNT_ID.dkr.ecr.us-east-1.amazonaws.com

# Build and push
REGISTRY="ACCOUNT_ID.dkr.ecr.us-east-1.amazonaws.com"
docker build -f services/coordinator/Dockerfile -t $REGISTRY/coordinator:latest .
docker push $REGISTRY/coordinator:latest
```

#### GCP (GCR):
```bash
# Configure Docker for GCP
gcloud auth configure-docker

# Build and push
REGISTRY="gcr.io/$GCP_PROJECT_ID"
docker build -f services/coordinator/Dockerfile -t $REGISTRY/coordinator:latest .
docker push $REGISTRY/coordinator:latest
```

### 2. Database Initialization

#### AWS RDS:
```bash
# Get RDS endpoint
RDS_ENDPOINT=$(terraform output -raw rds_address)

# Connect and initialize schema
psql -h $RDS_ENDPOINT -U coordinator -d coordinator -f scripts/init-db.sql
```

#### GCP Cloud SQL:
```bash
# Proxy to Cloud SQL
cloud_sql_proxy -instances=$GCP_PROJECT_ID:us-central1:stellpoker-cloudsql=tcp:5432 &

# Initialize schema
psql -h localhost -U coordinator -d coordinator -f scripts/init-db.sql
```

### 3. Deploy Application Services

#### AWS (ECS):
```bash
# Services are automatically deployed through Terraform
# Monitor via AWS Console or CLI:
aws ecs describe-services \
  --cluster stellpoker-cluster \
  --services stellpoker-coordinator-service \
  --region us-east-1
```

#### GCP (GKE):
```bash
# Deploy Coordinator to GKE
kubectl apply -f - << EOF
apiVersion: apps/v1
kind: Deployment
metadata:
  name: coordinator
spec:
  replicas: 2
  selector:
    matchLabels:
      app: coordinator
  template:
    metadata:
      labels:
        app: coordinator
    spec:
      serviceAccountName: stellpoker
      containers:
      - name: coordinator
        image: gcr.io/$GCP_PROJECT_ID/coordinator:latest
        ports:
        - containerPort: 8080
        env:
        - name: MPC_NODE_0
          value: "http://mpc-node-0:8101"
        - name: MPC_NODE_1
          value: "http://mpc-node-1:8102"
        - name: MPC_NODE_2
          value: "http://mpc-node-2:8103"
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: database-credentials
              key: url
EOF
```

## Monitoring and Maintenance

### AWS CloudWatch

```bash
# View logs
aws logs tail /ecs/stellpoker-cluster --follow

# List metrics
aws cloudwatch list-metrics --namespace AWS/RDS

# Get alarm status
aws cloudwatch describe-alarms --alarm-names "stellpoker-*"
```

### GCP Cloud Logging

```bash
# View GKE logs
gcloud logging read "resource.type=k8s_cluster" --limit 50

# View Cloud SQL logs
gcloud sql operations list --instance=stellpoker-cloudsql

# View Cloud Load Balancer logs
gcloud logging read "resource.type=http_load_balancer" --limit 50
```

## Scaling

### AWS Auto-Scaling

```bash
# Modify auto-scaling target
terraform apply -var="coordinator_desired_count=5"

# View scaling policy
aws application-autoscaling describe-scaling-policies \
  --service-namespace ecs
```

### GCP Auto-Scaling

```bash
# Resize GKE node pool
gcloud container node-pools update main-node-pool \
  --num-nodes=5 \
  --cluster=stellpoker-gke-cluster \
  --zone=us-central1-a

# Configure cluster auto-scaling
gcloud container clusters update stellpoker-gke-cluster \
  --enable-autoscaling \
  --min-nodes=1 \
  --max-nodes=10
```

## Disaster Recovery

### AWS Backups

```bash
# List RDS snapshots
aws rds describe-db-snapshots \
  --db-instance-identifier stellpoker-db

# Create manual snapshot
aws rds create-db-snapshot \
  --db-instance-identifier stellpoker-db \
  --db-snapshot-identifier stellpoker-db-manual-backup
```

### GCP Backups

```bash
# List Cloud SQL backups
gcloud sql backups list --instance=stellpoker-cloudsql

# Create on-demand backup
gcloud sql backups create \
  --instance=stellpoker-cloudsql

# Restore from backup
gcloud sql backups restore BACKUP_ID \
  --backup-instance=stellpoker-cloudsql
```

## Cost Optimization

### AWS Cost Optimization

1. **Use Fargate Spot**: Edit `aws_ecs.tf` to increase FARGATE_SPOT weight
2. **Scheduled Scaling**: Add time-based scaling for non-prod environments
3. **S3 Lifecycle**: CDN logs are auto-archived to Glacier after 30 days
4. **Reserved Instances**: Consider RDS Reserved Instances for production

### GCP Cost Optimization

1. **Preemptible Nodes**: Set in non-prod: `preemptible = true`
2. **Committed Use Discounts**: Available for GKE and Cloud SQL
3. **Scheduling**: Configure node auto-scaling with time-based policies
4. **Cloud CDN**: Cache static content to reduce origin requests

## Troubleshooting

### AWS Issues

**Problem**: ECS tasks failing to start
```bash
# Check task definition
aws ecs describe-task-definition \
  --task-definition stellpoker-coordinator

# View task logs
aws logs tail /ecs/stellpoker-cluster --follow

# Check service events
aws ecs describe-services \
  --cluster stellpoker-cluster \
  --services stellpoker-coordinator-service
```

**Problem**: RDS connection issues
```bash
# Verify security group
aws ec2 describe-security-groups \
  --group-names "stellpoker-rds-sg"

# Check RDS status
aws rds describe-db-instances \
  --db-instance-identifier stellpoker-db
```

### GCP Issues

**Problem**: GKE pod not starting
```bash
# Describe pod
kubectl describe pod POD_NAME

# Check logs
kubectl logs POD_NAME

# Check node status
kubectl get nodes -o wide
```

**Problem**: Cloud SQL connection issues
```bash
# Check Cloud SQL proxy
cloud_sql_proxy -instances=$GCP_PROJECT_ID:us-central1:stellpoker-cloudsql=tcp:5432 &

# Verify connectivity
psql -h localhost -U coordinator -d coordinator -c "SELECT 1"
```

## Cleanup

### AWS Cleanup

```bash
# Destroy all resources
terraform destroy -var-file="terraform.tfvars"

# Remove S3 state (CAREFUL!)
aws s3 rm s3://stellpoker-terraform-state-ACCOUNT_ID --recursive
aws s3api delete-bucket --bucket stellpoker-terraform-state-ACCOUNT_ID
```

### GCP Cleanup

```bash
# Destroy all resources
terraform destroy -var-file="terraform-gcp.tfvars"

# Delete GCP project
gcloud projects delete $GCP_PROJECT_ID
```

## References

- [Terraform AWS Provider](https://registry.terraform.io/providers/hashicorp/aws/latest/docs)
- [Terraform Google Provider](https://registry.terraform.io/providers/hashicorp/google/latest/docs)
- [AWS ECS Best Practices](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/ecs-best-practices.html)
- [GKE Best Practices](https://cloud.google.com/kubernetes-engine/docs/best-practices)
- [Terraform Best Practices](https://developer.hashicorp.com/terraform/cloud-docs/recommended-practices)
