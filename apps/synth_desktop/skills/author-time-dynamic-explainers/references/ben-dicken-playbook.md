# Ben Dicken diagram-production playbook

This is a source-backed production procedure, not merely a style description.
It was reconstructed from Ben Dicken's public behind-the-scenes thread and the
user-provided screenshot of his Cursor prompt.

## Observed sequence

1. Begin with several rough static diagrams in Excalidraw.
2. Settle the layout and the visual objective before animation work.
3. Screenshot the chosen static diagram and provide that image to the coding
   agent as the scene reference.
4. Name the visual and describe what the animation must demonstrate.
5. Tell the agent to match the established visual family and to reuse its
   connector, box, and component primitives.
6. Supply additional context and iterate for several prompting rounds.
7. Give the agent separate brand-inspiration assets distilled into design rules,
   then ask it to improve the scene using those rules.
8. Add special explanatory graphics only when the mechanism needs them.
9. Tune sizing and spacing repeatedly before publication.

## Verbatim prompt shown in the user-provided screenshot

The following text is transcribed verbatim from the screenshot supplied for
this skill:

> Okay, so now on to the next visual. This one should have the ID primary dash replicas, and here's an image that it should be based on
>
> ./src/blog/primary-replicas.png
>
> The visual style in terms of the boxes, the arrows, etc. should be the same as what is already used in the one for DNS and the one for the like popular architecture, some of the other visuals. And also ensure that you're using the various reusable components like the connectors for lines for for the for the various boxes and components etc and also there is some word describing what this visual should Do as a part of the image, so make sure and read that

Preserve the wording above as source evidence. For actual authoring, translate
it into this compact handoff template:

```text
Create the next visual with ID <visual-id>.
Base its layout and explanatory intent on <reference-image>.
Read the annotation embedded in that reference and implement the described
state change over time.
Match the established visual family used by <named sibling visuals>.
Reuse the existing connector, line, box, label, and system-object components.
Keep object identity stable across beats and end on an inspectable final state.
```

## Application to Workshop

- Store the rough reference with the task assets, not as decoration inside the
  final visual.
- Name two or three sibling visuals whose visual grammar should be inherited.
- Prefer shared declarative primitives supported by
  `diagram.systems.dynamic.v1`.
- Treat the reference annotation as behavioral acceptance criteria.
- Expect iterative capture review; the first implementation is a draft.

## Sources

- Behind-the-scenes thread:
  https://x.com/BenjDicken/status/2077826044531556681
- Designer-assets-to-skill workflow:
  https://x.com/BenjDicken/status/2075356836124053890
- Sharding animation:
  https://x.com/BenjDicken/status/2077435500408049861
- Sharding-particle animation:
  https://x.com/BenjDicken/status/2077474373909520676
- Massively parallel backup animation:
  https://x.com/BenjDicken/status/2083213032839385547
- Companion engineering article:
  https://planetscale.com/blog/massively-parallel-postgres-backups
