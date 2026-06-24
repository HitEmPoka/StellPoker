{{- define "mpc-node.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "mpc-node.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{- define "mpc-node.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "mpc-node.labels" -}}
helm.sh/chart: {{ include "mpc-node.chart" . }}
{{ include "mpc-node.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "mpc-node.selectorLabels" -}}
app.kubernetes.io/name: {{ include "mpc-node.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "mpc-node.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "mpc-node.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{- define "mpc-node.image" -}}
{{- $tag := .Values.image.tag | default .Chart.AppVersion }}
{{- printf "%s:%s" .Values.image.repository $tag }}
{{- end }}

{{/* Headless service name used for stable DNS per-pod hostnames. */}}
{{- define "mpc-node.headlessServiceName" -}}
{{- printf "%s-headless" (include "mpc-node.fullname" .) }}
{{- end }}

{{/* Name of the ConfigMap or Secret that holds party TOML files. */}}
{{- define "mpc-node.partyConfigName" -}}
{{- printf "%s-party-configs" (include "mpc-node.fullname" .) }}
{{- end }}
