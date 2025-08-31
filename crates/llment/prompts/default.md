{{ role() }}
{% include "instructions/focus.md" %}
{% include "instructions/task.md" %}
{% include "instructions/autonomy.md" %}
{% for file in glob("env/*.md") %}
{% include file %}
{% endfor %}
{% for file in glob("shell/*.md") %}
{% include file %}
{% endfor %}
