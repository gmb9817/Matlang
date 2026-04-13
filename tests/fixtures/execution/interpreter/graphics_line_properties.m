f = figure(40);
base = line([0, 1, 2], [0, 1, 0]);
styled = line('XData', [0, 1, 2], 'YData', [2, 1, 2], 'Color', 'r', 'DisplayName', "Styled");

base_type = get(base, 'Type');
base_color = get(base, 'Color');
styled_name = get(styled, 'DisplayName');
styled_color = get(styled, 'Color');

set(base, 'Color', [0, 0, 1]);
updated_base_color = get(base, 'Color');
red_lines = findobj(f, 'Color', [1, 0, 0]);
blue_lines = findobj(f, 'Color', [0, 0, 1]);
