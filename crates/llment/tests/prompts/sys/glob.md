{% for name in glob("sys/**/*.md") %}
{% if name != "sys/glob.md" %}
{% include name %}
{% endif %}
{% endfor %}
