{{ role() }}
{% include "snippets/instructions/focus.md" %}
{% include "snippets/instructions/task.md" %}
{% include "snippets/instructions/autonomy.md" %}
{% include "snippets/instructions/context.md" %}
{% for file in glob("snippets/env/*.md") %}
{% include file %}
{% endfor %}
{% for file in glob("snippets/shell/*.md") %}
{% include file %}
{% endfor %}
