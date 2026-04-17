function y = handle_receivers_codegen(objs, s)
f = @objs(:,2).total;
g = @objs.duplicate()(3).total;
h = @s.item.total;
y = f;
end
