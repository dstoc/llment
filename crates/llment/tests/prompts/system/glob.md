{% for name in glob("system/**/*.md") %}
{% if name != "system/glob.md" %}
{% include name %}
{% endif %}
{% endfor %}
