-- email.lua — responsive, Outlook-safe images for HTML email.
--
-- Gives every image an explicit `width` attribute and inline styles so it
-- never renders wider than the message column. This prevents two effects:
--   * Outlook (Word engine) ignores CSS `max-width`, so without a `width`
--     attribute a large image renders at its intrinsic size and blows the
--     layout wide.
--   * Mobile clients that "shrink to fit" scale the whole message (text
--     included) down to the widest element; a capped, fluid image keeps
--     the message within the viewport so the text stays its natural size.
--
-- MAX_WIDTH must match the body `max-width` in css/themes/email.css.
-- Usage: pandoc --lua-filter email.lua ...  (or lua-filter: email via the
-- mail CLI).

local MAX_WIDTH = "600"

function Image(img)
  local attrs = {}
  for _, kv in ipairs(img.attr.attributes) do
    if kv[1] ~= "width" and kv[1] ~= "height" and kv[1] ~= "style" then
      attrs[#attrs + 1] = kv
    end
  end
  attrs[#attrs + 1] = { "width", MAX_WIDTH }
  attrs[#attrs + 1] = { "style", "display:block; max-width:100%; height:auto; border:0;" }
  img.attr.attributes = attrs
  return img
end
