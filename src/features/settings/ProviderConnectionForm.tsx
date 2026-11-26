import { CreateProviderConnectionForm } from './CreateProviderConnectionForm';
import { EditProviderConnectionForm } from './EditProviderConnectionForm';
import type {
  CreateProviderConnectionFormProps,
  EditProviderConnectionFormProps,
} from './providerConnectionFormUtils';

export type ProviderConnectionFormProps = CreateProviderConnectionFormProps | EditProviderConnectionFormProps;

export function ProviderConnectionForm(props: ProviderConnectionFormProps) {
  if (props.mode === 'edit') {
    return <EditProviderConnectionForm {...props} />;
  }
  return <CreateProviderConnectionForm {...props} />;
}
