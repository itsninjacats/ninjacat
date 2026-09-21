{{- define "ninjacat.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Fully qualified app name. Truncated to 63 chars (DNS label limit), leaving
room for the longest component suffix we append ("-server-internal").
*/}}
{{- define "ninjacat.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 47 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 47 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 47 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{- define "ninjacat.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "ninjacat.labels" -}}
helm.sh/chart: {{ include "ninjacat.chart" . }}
app.kubernetes.io/name: {{ include "ninjacat.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels for one component. Call with (dict "ctx" $ "component" "server").
*/}}
{{- define "ninjacat.selectorLabels" -}}
app.kubernetes.io/name: {{ include "ninjacat.name" .ctx }}
app.kubernetes.io/instance: {{ .ctx.Release.Name }}
app.kubernetes.io/component: {{ .component }}
{{- end }}

{{- define "ninjacat.componentLabels" -}}
{{ include "ninjacat.labels" .ctx }}
app.kubernetes.io/component: {{ .component }}
{{- end }}

{{- define "ninjacat.serverImage" -}}
{{- printf "%s:%s" .Values.server.image.repository (.Values.server.image.tag | default .Chart.AppVersion) }}
{{- end }}

{{- define "ninjacat.frontendImage" -}}
{{- printf "%s:%s" .Values.frontend.image.repository (.Values.frontend.image.tag | default .Chart.AppVersion) }}
{{- end }}

{{- define "ninjacat.secretName" -}}
{{- .Values.secrets.existingSecret | default (printf "%s-secrets" (include "ninjacat.fullname" .)) }}
{{- end }}

{{- define "ninjacat.origin" -}}
{{- if .Values.frontend.origin }}
{{- .Values.frontend.origin }}
{{- else }}
{{- printf "%s://%s" (ternary "https" "http" .Values.ingress.tls.enabled) .Values.domainRoot }}
{{- end }}
{{- end }}

{{/*
Every intake hostname, one per line.
*/}}
{{- define "ninjacat.intakeHosts" -}}
{{- range .Values.ingress.intake.hostPrefixes }}
{{ printf "%s.%s" . $.Values.domainRoot }}
{{- end }}
{{- range .Values.ingress.intake.extraHosts }}
{{ . }}
{{- end }}
{{- end }}
