function y = control_flow_codegen(mode, x)
if mode > 0
    y = x + 1;
else
    y = x - 1;
end

for i = 1:2
    y = y + i;
end

switch mode
case 1
    y = y * 2;
otherwise
    y = y / 2;
end
end
