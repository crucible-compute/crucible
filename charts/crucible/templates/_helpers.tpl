{{/*
Expand the name of the chart.
*/}}
{{- define "crucible.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "crucible.fullname" -}}
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

{{/*
Common labels
*/}}
{{- define "crucible.labels" -}}
helm.sh/chart: {{ include "crucible.name" . }}-{{ .Chart.Version | replace "+" "_" }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: crucible
{{- end }}

{{/*
Selector labels for operator
*/}}
{{- define "crucible.operator.selectorLabels" -}}
app.kubernetes.io/name: crucible-operator
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Selector labels for API server
*/}}
{{- define "crucible.api.selectorLabels" -}}
app.kubernetes.io/name: crucible-api
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Namespace
*/}}
{{- define "crucible.namespace" -}}
{{- default .Release.Namespace .Values.namespace }}
{{- end }}

{{/*
Service account name
*/}}
{{- define "crucible.serviceAccountName" -}}
{{- if .Values.serviceAccount.name }}
{{- .Values.serviceAccount.name }}
{{- else }}
{{- include "crucible.fullname" . }}
{{- end }}
{{- end }}
